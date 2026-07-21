# ADR 0042 — Subgraphs on the catalog path + concurrent subgraph fan-out

- Status: **Proposed** — implementation ADR for product ADR 0068 (child workflows), signed off
  (merged, revised) at github.com/prxmat/adriane docs/adr/0068-child-workflows.md. Mirrors the
  ADR 0041 pattern: this ADR is the engine-side design; merge here = sign-off to implement.
- Date: 2026-07-21
- Deciders: Mathieu (owner)
- Relates to: product ADR 0068 (child workflows, the governance/control-plane design this
  implements the engine half of), ADR 0038 (replay-as-evidence), ADR 0027 phase 4b (`mapAgents`
  dynamic fan-out — extended here, not replaced)

## Context

Product ADR 0068 (revised 2026-07-21, after this repo was audited) found that child-graph execution
already exists end to end on the Rust engine, tested, but is unreachable from the catalog path the
product actually runs:

- `NodeType::Subgraph` + `NodeDefinition.subgraph_id` (`crates/graph-core/src/types.rs`) — already
  part of the core, wire-shared node schema.
- `execute_subgraph` (`crates/graph-runtime/src/runtime.rs:913`) — recursive start/resume sharing
  the parent runtime's checkpointer, suspend propagation to the parent (itself a checkpointed
  transition — sets `state.version += 1`, calls `self.suspend(...)`), failure propagation
  (`RuntimeError::SubgraphFailed`).
- `EngineSpec.subgraphs: Vec<GraphDefinition>` (`crates/runtime-bridge/src/spec.rs`) — the wire
  field the Rust bridge reads to resolve a `subgraph_id` reference. Round-trip tested.
- All of the above is exercised by a real `describeIfRust` test suite
  (`packages/graph-sdk/src/subgraph.test.ts`) — but only via the **builder** path
  (`GraphBuilder.subgraph()` → `CompiledGraph`, which threads `parts.subgraphs` into `EngineSpec`).

The **catalog** path — `runCatalogGraph` (`packages/graph-sdk/src/run-catalog-graph.ts`), what the
product's `RunsService` actually calls for every real run — hardcodes `subgraphs: []` at line 359.
`RunCatalogGraphOptions` has no `subgraphs` field to populate it with. This is the entire engine-
side gap for the SINGLE-child case (product ADR 0068 D2).

Product ADR 0068 D3 additionally wants a DYNAMIC fan-out of N independently-attested children (e.g.
"process each of these 40 invoices as its own governed sub-run") — the `mapAgents`-shaped UX, but
with subgraph children instead of agent children. This repo's `mapAgents` dispatch
(`crates/agents-core/src/node.rs:118`, `map_node_handler`) is hard-typed to `Arc<ReActAgent>`: it
calls `agent.run_scoped(item, ...)` per array item, `join_all`'d for concurrency, merged in input
order into the `join_at` channel. It has no path to invoke `execute_subgraph` instead of an agent —
confirmed by reading the function, not assumed. **This resolves product ADR 0068's flagged open
question**: D3 is NOT free: a genuinely new, parallel dispatch function is needed.

## Decision

### D1 — Expose `subgraphs` on `RunCatalogGraphOptions` (single-child case, product ADR 0068 D2)

Add `subgraphs?: GraphDefinition[]` to `RunCatalogGraphOptions` (and the matching `Pick<...>` on
`resumeCatalogGraph`'s options — `replayCatalogGraph` does not need it, since a subgraph node's
execution is itself journaled as part of the parent's replay just like any other node transition).
`run-catalog-graph.ts` threads it straight into `EngineSpec.subgraphs`, replacing the hardcoded
`[]` — the exact field `compiled-graph.ts:320` already populates for the builder path. No Rust
changes: `execute_subgraph`/`EngineSpec.subgraphs` already do the work; this is purely exposing an
existing capability through a currently-closed door.

A catalog graph authors a child call exactly like the builder path does: a node with
`type: "subgraph"`, `subgraphId` naming an entry in the `subgraphs` array. No new authoring surface
— `NodeType::Subgraph`/`subgraph_id` are already core `NodeDefinition` fields (unlike agent/
component nodes, not carrier-based metadata), so the catalog assembly (`run-catalog-graph.ts` on the
product side) needs no new node-metadata carrier, just to pass the `subgraphs` array through.

### D2 — `mapSubgraphs`: a new dispatch function mirroring `map_node_handler`, for N concurrent children

A new function, `crates/agents-core/src/node.rs`, alongside (not replacing) `map_node_handler`:

```rust
pub fn map_subgraph_handler(
    runtime: Arc<GraphRuntime>,   // the SAME runtime execute_subgraph already uses
    subgraph_id: SubgraphId,
    over_channel: String,
    join_at: String,
) -> NodeHandler
```

Mirrors `map_node_handler`'s exact shape: read `over_channel` as a JSON array, one child-run spawn
per item (deterministic id: `{parent_run_id}:{node_id}:{index}`, extending `execute_subgraph`'s
existing `{parent_run_id}:{node_id}` convention with an index suffix for the N-child case), run all
spawns concurrently via `futures_util::future::join_all` (same concurrency primitive, same
input-order-preserving merge guarantee `map_node_handler` already relies on), write each child's
terminal state (or its own suspension, propagated identically to the single-child case) into
`join_at` as a JSON array. A per-item failure is captured as `{ "error": ... }` at that index,
mirroring `map_node_handler`'s existing per-spawn error handling — never failing the whole node
silently.

This needs one carrier addition (mirrors `MapAgentCarrier`): a `mapSubgraphs` variant on the wire
(`node.metadata.mapSubgraphs = { subgraphId, overChannel, joinAt }`) alongside the existing
`mapAgents` carrier — a graph author picks one or the other per node, they are not composed.

### D3 — Concurrent suspension under fan-out: extend, don't reinvent

`execute_subgraph`'s existing suspend propagation (single-child case) sets the PARENT run suspended
when ONE child suspends. Under `map_subgraph_handler`'s N-concurrent-children case, if MULTIPLE
children suspend simultaneously (e.g. 3 of 40 invoices hit a human-gate), the parent suspends once,
carrying ALL outstanding children's suspended snapshots (extends the existing
`__subgraphStates`-keyed-by-child-run-id channel, which is already a map, not a single slot — no
schema change, just multiple entries populated instead of one). Resume re-attaches to every
outstanding child by iterating that map, exactly as the single-child path already does per key.

## Invariants (carried over from product ADR 0068, engine-enforced where noted)

1. **No new checkpoint semantics.** `map_subgraph_handler`'s node execution is itself one
   checkpointed transition on the parent, like any other node — concurrent children execute inside
   that one transition, not as separate checkpoint events on the parent's own stream.
2. **Approval gates evaluate fresh per child**, holds by construction — `execute_subgraph` (called
   once per fan-out item) reuses the SAME `approvalEngine` instance the parent run was given, and
   each child's own gated tool call carries its own `node_id`-scoped `requestedBy`/subject.
   Confirmed by reading the existing single-child seam; `map_subgraph_handler` doesn't change this.
3. **Deterministic child ids.** `{parent_run_id}:{node_id}:{index}` — stable across a resume (the
   control plane can always re-derive which id belongs to which array item), never a randomly
   generated id that would break resumability.

## Consequences

- `packages/graph-sdk`: `RunCatalogGraphOptions.subgraphs` (new, optional), `run-catalog-graph.ts`
  wiring (D1) — small, mechanical.
- `crates/agents-core`: new `map_subgraph_handler` (D2) — genuinely new code, but mirrors
  `map_node_handler`'s existing, tested shape closely; no new concurrency primitive (same
  `join_all`), no new suspend/resume primitive (extends `execute_subgraph`'s existing
  `__subgraphStates` map to carry N entries instead of assuming one).
- `crates/runtime-bridge`: dispatch a `mapSubgraphs` carrier to `map_subgraph_handler`, mirroring
  how `MapAgentSpec`/`build_map_agent_handler` are dispatched today (`lib.rs:1152`).
- `crates/graph-sdk` TS wire types: a `MapSubgraphCarrier` alongside `MapAgentCarrier`.
- Product-side control-plane work (giving each child a real `runsTable` row + attestation) is OUT
  OF SCOPE here — that is product ADR 0068's own remaining work, layered on top of this engine
  surface, not part of this ADR.

## Rejected

- **Generalizing `map_node_handler` to accept an arbitrary async closure** instead of a dedicated
  `map_subgraph_handler`. Rejected: `map_node_handler` is existing, tested, production code —
  refactoring its signature risks regressing the agent fan-out case for a feature that doesn't need
  it touched. A parallel, mirrored function is more code but zero risk to what already works.
- **A single unified "map" node kind covering both agents and subgraphs at authoring time.**
  Rejected for the same reason product ADR 0068 rejected merging `mapAgents` and the new subgraph
  fan-out at the product layer: they're deliberately two choices (cheap/disposable vs.
  governed/independently-attested), and conflating them at the wire-carrier level would obscure
  which one a graph author is actually getting.

## Build plan

1. D1 (small): `RunCatalogGraphOptions.subgraphs`, `run-catalog-graph.ts` wiring, a
   `describeIfRust` test proving a catalog-authored single-child subgraph resumes/attests correctly
   (mirrors `subgraph.test.ts`'s existing builder-path coverage).
2. D2/D3 (the real work): `map_subgraph_handler`, the `mapSubgraphs` carrier + TS type +
   `runtime-bridge` dispatch, concurrent-suspension handling. Tests: N-concurrent-children happy
   path, a subset suspending while others complete, a subset failing without failing the others'
   results, resume re-attaching to all outstanding children after a process restart.
3. Release + product repoints (standard flow) → product ADR 0068's control-plane recursion
   (`persistCatalogOutcome` walking newly-terminal child ids) becomes buildable.
