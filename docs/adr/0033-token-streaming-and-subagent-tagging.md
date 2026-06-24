# ADR 0033 — Per-token LLM streaming + nested sub-agent event tagging (deep-agent platform phase 13)

- Status: **Accepted** (signed off 2026-06-24). Touches an engine public API surface (`stream()` on `LlmGateway`/`LlmProviderAdapter`) and the `RunEvent` vocabulary — reviewed and approved.
- Deep-agent platform: [ADR 0023](0023-governed-deep-agent-platform-landscape.md) **phase 13** (the old "4c", deferred by [ADR 0027](0027-async-subagents-and-streaming.md) §4a).
- Builds on: [ADR 0027](0027-async-subagents-and-streaming.md) (4a wired node/tool lifecycle events; 4b `mapAgents` dynamic fan-out), [ADR 0014](0014-engine-token-efficiency.md) (the dormant `LlmStreamChunk` type), [ADR 0025](0025-unified-agent-middleware-api.md) (the middleware loop), [ADR 0032](0023-governed-deep-agent-platform-landscape.md) / phase 10 (the **durability ≠ observability** split — masked on the event clone, full in the checkpoint).

> Numbering note: ADR **files** on disk currently stop at 0028, but the journal's ADR sequence is already further along — phases 8/9/10 and the per-model-packages work claimed **0029** (structured output), **0030** (multimodal), **0031** (per-model packages) and **0032** (secrets) *in the journal narrative*; those four ADR files were never written (a documentation debt), but the numbers are taken. This work keeps the next number in that sequence, **0033** (as the phase-13 plan always called it), rather than reusing 0029.

## Context

[ADR 0027](0027-async-subagents-and-streaming.md) shipped **4a** (the Rust engine streams node + tool lifecycle events incrementally through `streamViaRust`, all four `StreamMode`s) and **4b** (`mapAgents` dynamic fan-out — one sub-agent per item, concurrent, deterministic input-order merge, human-gate preserved per spawn). It explicitly deferred **4c**: per-**token** LLM deltas and **nested sub-agent** event tagging.

**Grounded state today (re-verified 2026-06-24):**
- `LlmStreamChunk { delta: String, done: bool }` is defined and exported (`llm-gateway/src/types.rs:110-114`) but **never referenced** — dormant since ADR 0014.
- The provider trait `LlmProviderAdapter` (`llm-gateway/src/gateway.rs:11-15`) and `LlmGateway` (`:17-20`) have **`complete()` only** — no `stream()`. None of the real adapters (Anthropic/OpenAI-compatible/Gemini) stream; all call `.json()` once.
- `RunEvent` (`graph-runtime/src/types.rs:34-76`) has **7 variants**, no token variant. serde tags ride `tag="type"` + `rename_all_fields="camelCase"`, so a new variant + new fields serialize to the JS wire shape automatically. There are **no exhaustive `match` arms** over `RunEvent` in the crate (every site uses `_` or a single-variant `matches!`) — adding a variant is compiler-non-breaking on the Rust side.
- The napi `on_event` TSFN closure (`bindings/src/bridge.rs:507-512`) is generic: `serde_json::to_string(&event)` → non-blocking TSFN. Any new variant crosses to JS with **zero napi change**.
- `ReActAgent` is a builder struct (`agents-core/src/react.rs:95-128`); its sole LLM call is `self.gateway.complete(request.clone())` at `react.rs:243`. An `Option<EventSink>` field plugs in exactly like `with_middleware` (`:133-136`).
- `map_node_handler` (`agents-core/src/node.rs:106-152`) spawns one agent per item under `join_all`, merging in **input order** (`:124-127`) — but with **zero spawn-index or parent tracking**; all spawns share the mapAgents `node_id`.
- TS mirror: `RunEvent` union (`graph-runtime/src/types.ts:12-43`), Zod `RunEventDtoSchema` discriminated union (`contracts/src/events.ts:58-66`), and `shape()` projection (`graph-sdk/src/compiled-graph.ts:560-582`) — which today branches only on `node_completed` (per mode) + `debug`; **all other event types project to `[]`** (silently discarded). Comment at `:491`: "token-level deltas need gateway token streaming — still deferred".

A UI cannot show a live per-token view, and with concurrent `mapAgents` spawns sharing one `node_id`, it cannot demultiplex which token belongs to which sub-agent.

## Decision

Deliver per-token streaming as a **purely observational** capability, **opt-in**, **byte-identical** to the non-streaming path when no consumer is attached, and routed so token deltas **never enter durable state**.

### 13a — Real provider SSE token streaming (opt-in)

1. **Provider SSE, now (not chunk-once).** Add `stream()` to `LlmProviderAdapter` + `LlmGateway` (default impl adapts `complete()` to a single terminal item, so un-migrated adapters compile). Implement real SSE in all three adapters — Anthropic `messages.stream`, OpenAI-compatible `stream:true`, Gemini `:streamGenerateContent` — reusing the existing deterministic `build_request_body` / `to_response` mapping. Mock adapter gains a multi-chunk streaming mode for tests; `complete()` behavior preserved for all existing call sites.

2. **Opt-in via `Option<EventSink>`.** `ReActAgent` gains an `event_sink: Option<EventSink>` field (`with_event_sink()` builder). At the call site (`react.rs:243`): if a sink is attached, call `gateway.stream()` and emit each delta to the sink; otherwise call `complete()` unchanged. Gating is a per-run boolean threaded from the bridge (`build_react_agent`, `bridge.rs:633-657`). **A run with no token consumer takes the exact path it takes today.**

3. **`EventSink` bypasses the EventBus.** `EventSink` is a minimal trait defined in `agents-core` (no new dep on `graph-runtime`) and implemented in `bindings` to forward directly onto a clone of the `on_event` TSFN. Token deltas therefore reach JS over the **same wire** as lifecycle events but **never pass through `InMemoryEventBus.emit()`** (`graph-runtime/src/interfaces.rs:281-299`) — which is unconditional and would otherwise push every delta into the durable events vector. This single routing decision satisfies the durability exclusion (§Invariants 2) by construction.

4. **New observational `RunEvent::TokenDelta`.** `TokenDelta { run_id, node_id, delta, message_id, parent_run_id: Option<RunId>, spawn_id: Option<u32>, timestamp }`. It is a `RunEvent` variant for wire/type uniformity (rides the existing serde + TSFN + TS union), but it is produced only via the `EventSink`, never via `EventBus.emit()`, so it is structurally incapable of reaching a checkpoint or the event journal.

### 13b — Nested sub-agent tagging (structured fields)

5. **Structured `parentRunId` / `spawnId`, on `TokenDelta` only (this phase).** `map_node_handler` changes `.iter()` → `.enumerate()` (`node.rs:124-127`) to derive `spawn_id` = the input index (which equals the deterministic merge order at `join_at`), threaded into each spawned agent (`with_spawn_id()`). `parent_run_id` comes from the run state. These fields are **additive to** — not a replacement for — the existing `<parentRunId>:<nodeId>` RunId convention (`runtime.rs:172-179`), which the lifecycle events keep using. Adding the fields to the other lifecycle variants is **deferred** to phase 15 (Studio UI), when a concrete consumer needs them; it is a non-breaking additive change at that point.

### 13c — SDK projection (no new StreamMode)

6. **`shape()` branch.** `compiled-graph.ts:560-582` gains: `event.type === "token_delta" && mode === "messages"` → `[{ type: "message_delta", delta, nodeId, messageId }]`. `StreamMode` / `STREAM_MODES` / `StreamEvent` are **unchanged** — token deltas project onto the existing `message_delta` event. The TS `RunEvent` union and the Zod `RunEventDtoSchema` (`contracts/src/events.ts:58-66`) **must** gain a `token_delta` member — Zod is the one place a missing update is a *runtime parse failure*, not a silent drop.

## Resolutions to the open grounding questions (for sign-off)

| # | Question | Decision |
|---|---|---|
| 1 | `message_delta` requires a `messageId` — what id for token deltas? | **One stable id per agent turn** (`run_id`/`spawn_id` + the ReAct iteration index). All tokens of a turn share it; the client concatenates. |
| 2 | Add `parentRunId`/`spawnId` to all lifecycle events, or only `TokenDelta`? | **`TokenDelta` only** this phase (§13b). Lifecycle events keep the RunId convention; broaden in phase 15 when a UI consumes it. |
| 3 | Fix the pre-existing `run_completed` Rust↔TS divergence here? | **No — out of scope, documented.** Rust `RunCompleted{run_id,timestamp}` carries no `finalState` while TS/Zod require it (SDK reconstructs from `runPromise`). The ADR does **not** claim the unions are byte-identical. Filed as a separate cleanup follow-up. |
| 4 | How does the sink reach JS without `agents-core` → `graph-runtime` dep? | **Sink trait in `agents-core`, impl in `bindings` onto the `on_event` TSFN** (§13a.3). Also delivers the durability exclusion for free. |
| 5 | `LlmStreamChunk` carries no usage / tool-call finality. | The `stream()` terminal yields the **fully assembled `LlmResponse`** (content + usage + tool_calls + stop_reason), identical to what `complete()` returns. The agent uses that terminal response as authoritative; per-token deltas are emit-only. Tool-call/`functionCall` blocks surface only on the terminal, never as partial deltas. |
| 6 | Mock streaming shape? | Extend `MockAdapter` with an optional per-call stream script; absent a script, `stream()` chunks the `complete()` response into one terminal item. Every existing `MockAdapter::new(provider, vec![response])` call site is preserved. |

## Invariants (must hold)

1. **Byte-identical default + determinism.** With no sink, the path is unchanged. With a sink, the authoritative `LlmResponse` is the stream's terminal assembly — identical in every field to `complete()`. So `after_model` (`react.rs:245-248`), usage accumulation (`:250-258`), `content.trim()` (`:260`), conversation history, and every checkpoint are byte-identical. Replay produces identical state.
2. **Durability ≠ observability.** `TokenDelta` reaches observers via the `EventSink` only; it never touches `EventBus.emit()`, the durable events vector, the journal, or a checkpoint. A run interrupted mid-stream resumes by re-running the iteration (fresh `stream()`/`complete()`) — the checkpoint never recorded partial tokens. (Same split as the phase-10 no-log marker: masked on the event, full in state.)
3. **Concurrent spawns stay demultiplexable.** `mapAgents` spawns interleave on the shared TSFN in nondeterministic real-time order — which does **not** affect determinism (deltas are excluded from durable state). The SDK reorders/groups by `spawnId` (= input index = merge order), so a live view is correct despite racy wire arrival.
4. **Streaming stays audited.** The audit journal is unchanged: it is still built from the durable lifecycle events. Token streaming is a read view, not a side channel that escapes governance — the governed `LlmResponse` (post-redaction, post-approval) is the authoritative artifact; deltas are a projection of the same generation.

## Touch-points (grounded)

- Rust: `gateway.rs:11-20,38-46` (trait + default + routing); `anthropic.rs`/`openai_compatible.rs`/`gemini.rs` (SSE siblings); `mock.rs:11-43`; `types.rs:34-76` (`TokenDelta`); `react.rs:95-128,243-260` (sink field + builder + branch); `node.rs:124-127` (`enumerate` → `spawn_id`); `bridge.rs:507-512,633-657,688-712` (sink impl onto TSFN + thread the opt-in flag + spawn context).
- TS: `types.ts:12-43` (+`token_delta`); `contracts/src/events.ts:58-66` (+`TokenDeltaSchema`); `compiled-graph.ts:560-582` (projection branch), delete the `:491` deferral comment. `StreamMode`/`StreamEvent` schemas unchanged.

## Reserves / next

- Phase 15 (Studio nested sub-agent stream UI) consumes these events; it is the trigger to broaden `parentRunId`/`spawnId` onto lifecycle variants (resolution #2).
- The `run_completed` Rust↔TS divergence (resolution #3) is a standalone cleanup.
- Phase 14 (ACP / Google ADK) maps this token stream onto the protocols' streaming surface.
