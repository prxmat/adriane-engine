# ADR 0008 — Rust runtime parity: concurrent fan-out, subgraphs, streaming over napi

- Status: Proposed
- Date: 2026-06-20
- Builds on: [ADR 0003](0003-ts-engine-deprecated-sdk-on-rust.md) (Rust is the canonical engine; the TS runtime is the deprecated fallback)

## Context

ADR 0003 made the Rust engine (`engine/crates/graph-runtime`) the canonical executor and the TS engine (`engine/packages/graph-runtime`) a deprecated fallback. But the Rust port still lagged the TS runtime on three features that become limiting the moment the Studio orchestrates richer graphs (parallelism, sub-pipelines, live progress):

1. **Fan-out was sequential and semantically wrong.** The Rust port ran a node's `parallel_to` branches one after another, and — worse — fed each branch the *accumulated* channels (branch N+1 saw branch N's writes). The TS `executeFanOut` runs branches concurrently off the **pre-fan-out snapshot** (map-reduce), merging after. So Rust both serialized the work and diverged on semantics.
2. **Subgraphs were absent.** `NodeType::Subgraph` and `subgraphId`/`inputMapping`/`outputMapping` existed in the model but the Rust runtime had no execution for them (`// Deferred: subgraphs`). The TS runtime ran child graphs, mapped channels in/out, and propagated child suspension.
3. **Streaming had no Rust path.** The Rust runtime emits raw lifecycle events via `on_event` (and the napi bridge already forwards them to JS), but the SDK's four `stream()` modes (`values` / `updates` / `messages` / `debug`) were TS-only; on the Rust path `stream()` degraded to a single terminal `state_value`.

Determinism is a hard runtime invariant (checkpoint after every node, event per transition, reproducible state). Making fan-out concurrent must not break it; adding subgraphs must preserve clean suspend/resume.

## Decision

### 1. Fan-out runs concurrently, merges deterministically

`execute_node` now launches all branch handlers concurrently with `futures_util::future::join_all` (`futures-util` is the only new dependency — executor-agnostic, so the crate stays async-runtime-agnostic). Determinism is preserved by two rules, documented at the call site:

- **Shared snapshot.** Every branch handler is called with the SAME pre-fan-out state clone. No branch observes a sibling's mid-flight write — map-reduce, not a chain. (This also fixes the prior semantic divergence from TS.)
- **Declared-order merge.** Branch updates are folded into the channels in `parallel_to` order, regardless of which future settles first. So `append`/`merge` reducers produce identical state on every run.

Proven by tests: a 2-party barrier rendezvous (sequential execution would deadlock → proves real concurrency), a declared-order `append` merge, and a pre-fan-out snapshot isolation check. The existing `Send + Sync` compile-time proof still holds (`join_all` keeps the run future `Send`).

### 2. Subgraphs execute via a threaded graph context

A `subgraph`-type node runs a registered child graph to completion or suspension, then maps channels back and routes on in the parent — mirroring the TS subgraph branch.

- **Shared seams, threaded graph.** The executor's private methods take a `GraphCtx { graph, node_by_id }` parameter instead of always reading `self.graph`. The node/condition registries, checkpointer and event bus stay shared on `self` (matching TS, where the child `GraphRuntime` is built from the parent's seams). Public `start`/`resume` delegate to `start_with_ctx`/`resume_with_ctx` with the top-level context; a subgraph node calls them with the **child's** context. The public API (`new`, `start`, `resume`, `update_state`, `replay_from`, the `events()`/`checkpointer()` accessors) is unchanged; child graphs are registered via a new `with_subgraphs(Vec<GraphDefinition>)` builder.
- **Channel mapping.** `apply_input_mapping` / `apply_output_mapping` port the TS semantics exactly (no mapping → copy all / spread-child-over-parent; with mapping → per-key projection).
- **Suspension propagation.** If the child suspends (e.g. an internal human gate), the parent suspends "during" at the subgraph node, recording the child run id in the `__subgraphRuns` channel. A later parent resume re-enters the node, finds the child checkpoint (shared checkpointer), and **resumes** the child rather than restarting it. Recursive child runs are boxed (`Box::pin`) to break the async-recursion cycle.

Proven by tests: a subgraph completing with in/out mapping, and a subgraph with an internal human gate suspending the parent then resuming to completion.

### 3. Subgraphs cross napi end-to-end; streaming shaping stays in the SDK

- **napi carrier.** `EngineSpec` gains `subgraphs: Vec<GraphDefinition>` (`#[serde(default)]`, back-compat). `build_runtime` registers the parent's **and** every subgraph's node handlers / conditions (child node ids are flattened into the same `jsNodeIds`/`agents`/`componentNodes` maps, keyed by global node id) and calls `with_subgraphs`.
- **SDK builder.** `GraphBuilder.subgraph(id, childBuilder, { inputMapping, outputMapping })` nests a child graph: it merges the child's wiring into the parent (shared registries, by global node id — colliding ids are a hard error), records the child `GraphDefinition`, and emits a `subgraph` node. `CompiledGraph` carries the children to Rust (`EngineSpec.subgraphs`) and to the TS engine (a `subgraphResolver`). Proven on both engines (run + internal-gate suspend/resume).
- **Cross-napi child resume (solved).** Each napi call rebuilds the checkpointer, so a child suspended on a prior call is absent from it. The engine carries the child's suspended snapshot in the parent state under `__subgraphStates[childRunId]` (written on the suspend path) and re-seeds the checkpointer from it before resuming — so a subgraph that suspends on an internal gate resumes cleanly across napi calls, not just in-process. No wire change (it rides the existing `state` round-trip); the snapshot is dropped once the child completes.
- **Streaming.** Raw run events already cross napi (`on_event` → SDK event bus). The four `stream()` modes are pure, deterministic projections over that feed and stay **SDK-side** (no engine logic). `updates` (a `state_update` per node completion) and `debug` (every event) are now streamed **incrementally** while the run is in flight; `values` (full snapshot per step) and `messages` (token deltas) need delta-accumulation / gateway token streaming and yield a single terminal `state_value` for now.
- **RunEvent wire fix.** The Rust `RunEvent` serialized its fields snake_case (`run_id`/`node_id`) while the TS `RunEvent` type the SDK parses is camelCase — so `event.nodeId` was `undefined` on the JS side (a latent bug streaming surfaced). `RunEvent` now uses `rename_all_fields = "camelCase"` (variant tags stay snake_case); a Rust test asserts the shape.

## Consequences

- **Public API (Rust).** `GraphRuntime::with_subgraphs(Vec<GraphDefinition>)` is new; existing constructors/methods/accessors are unchanged. `RuntimeError` gains `SubgraphNotResolvable` / `SubgraphNotFound` / `SubgraphFailed`. New dependency: `futures-util` (default-features off, `alloc`).
- **Public API (napi).** `EngineSpec.subgraphs` is new (`#[serde(default)]` → start/resume/approve specs that omit it still deserialize). The native addon must be rebuilt (`pnpm napi:build`).
- **Public API (SDK).** `GraphBuilder.subgraph(...)` is new; `CompiledGraphParts.subgraphs` / `RustRunnerParts.subgraphs` / `EngineSpecWire.subgraphs` carry them. `CompiledGraph.stream()` now streams `updates`/`debug` incrementally on the Rust path.
- **Public wire (events).** `RunEvent` fields serialize camelCase now (was snake_case). Any consumer that read `run_id`/`node_id` off a forwarded event must read `runId`/`nodeId` (which is what the TS type always declared). The addon must be rebuilt.
- **Behaviour.** Fan-out is now genuinely parallel on the canonical engine, with deterministic state — a behaviour change for any graph that relied (incorrectly) on the prior sequential accumulation. Subgraphs (incl. internal-gate suspend/resume across napi) are a new capability on the Rust engine and CLI; the TS engine already had them.
- **Invariants preserved.** Checkpoint after every node, one event per transition, clean suspend/resume — unchanged. Fan-out determinism is now an explicit, tested contract.
- **Update:** all four stream modes now project incrementally over the Rust feed — `values` accumulates a full snapshot per node via the channel reducers (SDK-side), `messages` emits a message-level `message_delta` per new `messages` entry. The `send` dynamic-message primitive also landed. **Still deferred:** token-level `messages` deltas (need gateway token streaming).
