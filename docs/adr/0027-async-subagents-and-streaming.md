# ADR 0027 — Async sub-agents + wired event streaming (deep-agent platform phase 4)

- Status: **Accepted** (signed off 2026-06-23). Decisions: ship **4a + 4b together** in one phase-4 PR; 4b uses a new **`.mapAgents(overChannel, subAgent, joinAt)`** dynamic-fan-out builder method (merge by input index = deterministic); LLM token-delta streaming stays deferred to 4c.
- Deep-agent platform: [ADR 0023](0023-governed-deep-agent-platform-landscape.md) **phase 4**.
- Builds on: [ADR 0017](0017-fan-out-builder.md) (`.fanOut()` static scatter/join), the `send`/inbox seam, [ADR 0014](0014-engine-token-efficiency.md) (`LlmStreamChunk`), [ADR 0025](0025-unified-agent-middleware-api.md) (the middleware loop), the napi event bridge.

## Context

Phases 1–3 shipped the deep-agent core: planning (`writeTodos`), sub-agents (`taskNode`), the
governed filesystem, and the unified middleware API. Two capabilities on the ADR 0023 wishlist
remain, both **structural** (runtime + napi):

**What exists today (grounded in the code, 2026-06-23):**
- **Concurrency** — `.fanOut(from, parallelTo[], joinAt)` already runs branches **concurrently**
  (`runtime.rs` builds `branch_futures` and awaits `futures_util::future::join_all`), merging in the
  **declared order** so `append`/`merge` reducers stay deterministic. But the fan-out list is
  **static** (fixed node ids). A **dynamic-N** map (spawn one sub-agent per runtime item) is only
  expressible via `send`/inbox, which drains **sequentially**.
- **Streaming** — `LlmStreamChunk` is typed and the native adapters (Anthropic, Gemini) can stream;
  the napi bridge forwards run **events** to JS through a threadsafe `on_event` callback. But
  `CompiledGraph.stream()` on the Rust path drives a full run and yields a **single terminal**
  `state_value` event — there is no incremental surface. Only the in-process TS engine streams
  incrementally.

## Decision

Deliver phase 4 as two independently-shippable sub-phases, both **governed by construction**
(every spawn is an audited node + checkpoint; a spawn that gates suspends the whole run):

### 4a — Wired event streaming (lower risk, ship first)
Thread the run's **lifecycle events** (already emitted on the EventBus + already forwarded over the
napi `on_event` seam) into `CompiledGraph.stream()` so it yields **incrementally** on the Rust path
— `node_started` / `node_completed` / tool-transcript / `run_suspended` / `run_completed` — instead
of one terminal event. No new engine concurrency; this is wiring the existing event stream through
to the existing `StreamMode` surface (`values` / `updates` / `messages` / `debug`).
- **LLM token-level** streaming (per-token deltas via `LlmStreamChunk` through napi) is **deferred to
  4c** — it is heavier (a per-token threadsafe hop) and not required for a useful live view.

### 4b — Async / dynamic sub-agents
A **dynamic fan-out**: spawn one sub-agent per runtime item (count known only at run time), executed
concurrently with the same deterministic merge guarantee as static `fanOut` (collect results, fold
in a stable sorted order so reducers are deterministic and the run is **resumable**). The
human-gate invariant is preserved **per spawn**: if any spawned sub-agent suspends for approval, the
whole run suspends cleanly and resumes from the latest checkpoint. Surfaced as a builder method
(e.g. `.mapAgents(overChannel, subAgent, joinAt)` — exact name TBD) that compiles to the dynamic
fan-out the runtime executes.

## Invariants (must hold)
1. **Determinism + resumability** — concurrent results merge in a stable order (declared, or sorted
   by a stable key), so a replay produces byte-identical state. A suspended spawn suspends the run;
   resume re-enters from the checkpoint. No wall-clock / `Math.random` ordering.
2. **Governed per spawn** — each sub-agent is an audited node with its own checkpoint; the approval
   gate (intrinsic, ADR 0025) applies inside each spawn.
3. **Streaming stays audited** — streamed events are the same EventBus events that form the audit
   journal; streaming is a read view, not a side channel.

## Open decisions (for sign-off)
1. **4a scope** — node/tool lifecycle events only now, LLM token deltas deferred to 4c? (Recommended:
   yes — ship the useful live view first, defer the heavy per-token hop.)
2. **4b primitive** — a new `.mapAgents()` dynamic-fan-out builder method (cleanest), vs extending
   `send`/inbox to drain concurrently (reuses the inbox, but changes its sequential semantics)?
3. **4b ordering key** — merge spawned results by **input order** (the channel array index) — confirm
   that is the deterministic key (vs a user-supplied key).
4. **Sequencing** — ship **4a first** (lower risk, immediately useful), then **4b**? (Recommended.)

## Reserves / next
- Token-level streaming (4c) + the Studio nested sub-agent stream UI (phase 5) consume 4a's wired
  events.
- Protocols (ACP / Google ADK, phase 5) expose the governed agent + its stream over a standard.
