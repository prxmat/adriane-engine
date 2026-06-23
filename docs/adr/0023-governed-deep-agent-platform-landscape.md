# ADR 0023 — Governed deep-agent platform: full capability landscape vs LangChain `deepagents`

- Status: Proposed (strategic landscape + phasing; no code commitment yet)
- Date: 2026-06-22
- Deciders: Mathieu (owner)
- Extends: [ADR 0022](0022-deep-agent-harness-gap.md) (core three: writeTodos / task / virtual fs)
- Related: [ADR 0013](0013-llm-council-governed-deliberation.md) (governed deliberation),
  [ADR 0014](0014-token-efficiency.md) (context budget / compression / caching),
  [ADR 0018](0018-model-tiering.md) (tiering → profiles),
  [ADR 0021](0021-opentelemetry-export-seam.md) (export seam pattern)

## Context

ADR 0022 scoped the *core three* deep-agent primitives (a planning tool, a sub-agent spawn
tool, a virtual filesystem). Reviewing the **full** LangChain deep-agent + agent-platform
surface — Skills, sandboxes/interpreters, context engineering, pluggable backends, async
sub-agents, permissions, memory, profiles, event streaming, a streaming frontend, prebuilt
middleware, and interop protocols (ACP / Google ADK) — shows a much larger landscape than 0022
captured. This ADR maps **all** of it against Adriane, decides **where each capability lives**
(engine primitive / external seam / control-plane / Studio product), and sets the **governed**
through-line and a phasing order. None of this surface exists in the engine today (verified): it
is greenfield beyond the existing reusable bricks (`principal` RBAC, `memory-store`, `EventBus`,
`LlmStreamChunk` type, `prebuilt-agents`, ADR 0014 compression/cache).

## Capability landscape

| Capability (deepagents / LangChain) | Adriane today | Gap | Lives in | Governed angle |
| --- | --- | --- | --- | --- |
| **writeTodos** (plan-in-state tool) | plan-execute pattern | agent-callable todo tool | `agents-core` | plan checkpointed + audited |
| **task** (sync sub-agent → compressed report) | subgraphs, supervisor, swarm, council | first-class spawn tool | `agents-core` | each spawn = audited node |
| **async sub-agents** (concurrent, stream back) | fan-out / `send` (parallel) | concurrent spawn + result streaming | `agents-core` + runtime | **human gate preserved** per spawn |
| **context engineering** (summarize/trim/offload) | working-memory compression, prompt caching + LLMLingua, `context_budget` (0014) | summarization-as-middleware | `agents-core` (extends 0014) | budget is config, not a prompt hack |
| **memory** (long-term, cross-thread) | `memory-store` (pluggable backends) | always-loaded `AGENTS.md` + cross-session wiring | `memory-store` | attributable, scoped store |
| **skills** (`SKILL.md` progressive) | KB / RAG + system-prompt path | the progressive-skill convention | KB + system prompt | skills versioned + governed |
| **virtual filesystem** (ls/read/write/edit/glob/grep) | `artifact-store` (versioned artifacts) | agent-operable fs | **`fs` seam** (0022 phase 2) | write/exec on guarded path → gate |
| **backends** (pluggable fs/state: local/store/composite) | `artifact-store` (single backend) | backend choice for the `fs` seam | `fs` seam | backend = policy decision |
| **sandbox / interpreters** (run code/shell) | — | isolated code execution | **external gated seam** (never in-engine) | exec **always** approval-gated |
| **permissions** (per-tool / per-path allow·deny·interrupt) | RBAC `principal`, ApprovalEngine, no-self-approval, signed audit | a per-tool/per-path rule DSL over tool calls | `agents-core` + `approval-engine` | **Adriane wins**: perms → gates + audit |
| **profiles** (preset agent configs) | `prebuilt-agents`, model tiering (0018) | named, shareable profile bundles | `agents-core` | profile = traced config |
| **prebuilt middleware** (summarization/HITL/PII/retry…) | scattered seams: `RedactingLlmGateway`, `CompressingGateway`, reflection | a **unified middleware composition API** | `agents-core` | governance becomes composable |
| **event streaming** (token / tool / sub-agent events) | `EventBus` (node lifecycle) + `LlmStreamChunk` (type, unwired) | wired streaming tool-transcript + SSE | runtime + bindings (follow-up) → API | streamed events stay audited |
| **frontend sub-agent streaming** | — | nested sub-agent stream UI | **Studio (product)** | — |
| **protocols (ACP / Google ADK)** | — | interop adapter (external clients drive/stream the agent) | **control-plane / API (seam)** | a governed agent, exposed via a standard |

## Decision

Adopt the deep-agent + agent-platform conveniences, but make Adriane's version **governed by
construction**: *every powerful action — filesystem write, code/shell execution, sub-agent spawn,
tool call — routes through permissions → an approval gate → signed audit → a checkpoint.* This is
the same bet as the council (0013): not a new trick, the **governed** version of a known-good
pattern. deepagents offers a bare `interrupt_on`; Adriane offers permissions + gates + audit +
sovereignty.

Placement rules (where each capability lives):

1. **Engine primitives (`agents-core` / seams)** — `writeTodos`, `task` (sync + async), the
   `fs` seam + pluggable backends, the permissions DSL, profiles, and a **middleware composition
   API** that unifies today's scattered seams (Redacting/Compressing gateways, reflection,
   summarization, HITL) into one composable surface. Skills/memory map onto KB + `memory-store`.
2. **External gated seams (never in-engine)** — **sandbox** and **interpreters** (code/shell
   execution). The OSS security rule forbids `eval`/exec of strings in-engine; execution is an
   external, approval-gated service behind an env-configured seam (the PII / LLMLingua / OTel
   pattern). `execute` always passes an approval gate.
3. **Control-plane / API seam** — interop **protocols (ACP, Google ADK)**: an adapter that lets
   external clients (editors, orchestrators) drive and stream a *governed* Adriane agent over a
   standard. Strategic interop, not an engine concern.
4. **Studio (product)** — the **frontend sub-agent streaming** UI consumes the engine's streamed
   events; rendering is a product feature, not engine.

The **unified middleware API** is the keystone abstraction: it turns each governance seam
(redaction, compression, HITL, summarization, retry, permissions) into a composable middleware,
so "governed deep agent" = a default middleware stack rather than bespoke wiring.

## Phasing

1. **Phase 1 (cheap, engine) — ✅ IMPLEMENTED.** `writeTodos` (a pure planning tool →
   durable `__todos` channel, sunk in the same checkpoint as the result) + `task`
   (`GraphBuilder.taskNode`, sugar over the existing subgraph node → checkpoint / audit /
   human-gate inherited, zero new runtime path). Parity Rust ↔ TS, additive wire
   (`AgentResult.todos` is optional), carrier wired on both the in-process and
   persisted/catalog paths. Adversarially reviewed (5 lenses); the only real finding — a
   pre-existing carrier gap also affecting the ADR 0014 knobs — was fixed in the same
   change.
2. **Phase 2 (the real build) — ✅ IMPLEMENTED ([ADR 0024](0024-governed-virtual-filesystem-seam.md), Accepted).** The governed `fs` seam (8 tools) + pluggable backends (artifact + durable HTTP, fail-closed) + the per-path permission DSL (`deny`<`read`<`gate`<`write`) → approval-gate integration with content-scoped grants. Phases 2a–2e shipped.
3. **Phase 3 (composition) — ✅ IMPLEMENTED ([ADR 0025](0025-unified-agent-middleware-api.md), Accepted).** The unified **middleware API**: one `AgentMiddleware` trait (7 hooks) + `MiddlewareStack` (governed/efficiency, onion); folded the scattered seams in (redaction, compression, terse, context-budget); the **approval gate is intrinsic** to `before_tool`; **profiles** (`fast`/`frontier-careful`/`governed-deep`) + user `middleware[]` ride on it (3d); **reflection** middleware (3e). Governed-by-construction: a governance kind in user data is unrepresentable.
4. **Phase 4 (async + streaming) — ⏳ NEXT.** async sub-agents (concurrent spawn, human-gate preserved per spawn) + wired event streaming (tool-transcript → SSE; `LlmStreamChunk` is typed but unwired). Touches the runtime + napi bridge → **own detailed ADR + sign-off** (structural).
5. **Phase 5 (interop + UI)** — ACP/ADK protocol adapter (control-plane) + Studio sub-agent
   streaming UI.
6. **Phase 6 — skills/memory** — long-term cross-thread agent memory over `memory-store` (the four memory planes M1–M4, [ADR 0026](0026-memory-architecture-engine-studio.md), Proposed) + the progressive-skill (`SKILL.md`) convention over the KB. Best surfaced as a `MemoryMiddleware` (`before_run`/`after_run`) now that the middleware API exists.

### Integrations taxonomy (the doc-site surface)

Every seam above is documented as a splittable **integration** (LangChain-style), so a reader picks a concrete piece: **models** (native Anthropic + Google Gemini; OpenAI-compatible for OpenAI / Azure / Mistral / OpenRouter / Groq / Hugging Face / Ollama / NVIDIA; AWS Bedrock via proxy/planned), **middleware**, **backends** (fs), **checkpointers**, **retrievers**, **text splitters**, **vector stores**, and **sandboxes** (external gated seam, planned). Adding a provider/seam of an existing family is a constructor + an enum slot, not a new integration (ADR 0005).

## Consequences

- A coherent product story: **"governed deep agents"** — the Claude-Code-shaped platform with
  permissions, approval gates, signed audit, RBAC, sovereignty, and the lightest orchestration
  (the benchmarks).
- The middleware API and the `fs` seam are the two big new abstractions; everything else is
  ergonomics over existing primitives or an external/edge seam.
- Engine stays lean: sandbox/interpreters/protocols never bloat the OSS core — they are seams.

## Reserves / next

- Large surface — do **not** build at once; the phasing is the contract. Phases 2 (fs+perms) and
  the sandbox/interpreter seam are security-relevant → each gets its own ADR + explicit sign-off
  before code (mandatory review).
- Open decision per 0022 stands: engine ships the primitives; the opinionated harness assembly is
  a prebuilt (recommended).
- ACP/ADK: pick one protocol to prototype first (ACP looks closer to the editor-client shape);
  ADK parity is a later interop bet.
- Async sub-agents must not bypass the human gate — the gate fires per spawn, even concurrent
  (the invariant that differentiates us; verify with a suspend/resume test under fan-out).
