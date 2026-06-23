# ADR 0029 — Governed structured output (deep-agent phase 8)

- **Status**: Accepted (design signed off 2026-06-23; key decisions D1–D4 confirmed by the project owner)
- **Context**: deep-agent harness phase 8 of [ADR 0023](0023-governed-deep-agent-platform-landscape.md)
- **Supersedes / relates to**: builds on the unified middleware API ([ADR 0025](0025-unified-agent-middleware-api.md)), the observability/audit bet ([ADR 0028](0028-observability-otel-cost.md)), and the model-tiering / provider family ([ADR 0005], [ADR 0018](0018-model-tiering-policy.md)).

## Context

Phase 8 of ADR 0023 is scoped in one line: *"No `response_format`/`json_schema`-constrained generation today (tools give structured input only). Add a structured-output API (native Anthropic / Gemini / OpenAI), validated against a schema."* Both halves — native constrained generation **and** schema validation — are mandatory; "validated against a schema" holds even on the native path.

The gap today is real and twofold (grounded in the code):

1. **No typed final answer.** The Rust ReAct loop produces the final answer as a `final:<answer>` substring appended to a `trace` vector (`crates/agents-core/src/react.rs:324-335`); `AgentResult` has **no** `output` field — only `reasoning`, `approval_requests`, `requires_human_review`, optional `todos`, optional `usage` (`react.rs:63-78`). The answer is recoverable only by parsing the `final:` line back out of `reasoning` (`react.rs:338-344`). The agent node sink serializes the **whole** `AgentResult` into the `agentResult` channel via a single patch (`crates/agents-core/src/node.rs:57-77`) — there is no slot for a typed answer.

2. **No schema-constrained generation at the gateway.** `LlmRequest` carries only `provider/model/messages/system/tools/max_tokens/temperature` — **no** `response_format`, **no** `tool_choice`, **no** `json_schema` (`crates/llm-gateway/src/types.rs:81-95`). Tools are passed but the model is never *forced* to call one (no `tool_choice` in any adapter). There is **no** JSON-Schema validation library anywhere in the repo (no `jsonschema`/`schemars`/`ajv`); all validation today is Zod (graph model / DTOs only) or the flat `jsonValidator` (top-level type + flat `requiredKeys`, `crates/components/src/components.rs:267-351`).

Strong reuse anchors exist: `LlmToolDef.input_schema` is already a free-form `serde_json::Value` JSON Schema (`types.rs:61-68`); `LlmToolCall.input` is already a parsed `Value`; the `AgentMiddleware` trait already exposes `before_model` (rewrite request), `after_model` (**fallible** — `Err` short-circuits, fail-closed) and `after_run` (`&mut result`) hooks (`crates/agents-core/src/middleware.rs:76-144`); and `usage`/`todos` are the established **additive-optional-`skip_serializing_if`** field precedent (ADR 0028 phase 7a). The approval gate is **intrinsic** to `MiddlewareStack::before_tool`, evaluated before any installed middleware (ADR 0025 / phase 3c) — so no installed middleware can defeat it.

The invariants this feature must not break: checkpoint-after-every-node-completion and one-patch-one-checkpoint (`node.rs:45-72`, `crates/graph-runtime/src/runtime.rs:754-766`); determinism / no-clock (`interfaces.rs:39-45`); and the sealed governed layer + intrinsic gate.

## Decision

Adopt **governed structured output** as a purely **additive, opt-in** feature whose authority lives in the **Rust engine**, expressed through the existing middleware + additive-optional-field conventions — never a structural rewrite. Four owner-confirmed choices (D1–D4):

### 1. Generation: provider-native first → forced-tool-use fallback → always validate (D3)

Add a provider-neutral optional field to `LlmRequest`:

```rust
pub enum ResponseFormat {
    JsonSchema { name: String, schema: Value, strict: bool },
}
// LlmRequest { …, response_format: Option<ResponseFormat> }
```

reusing the `serde_json::Value`/`input_schema` convention rather than inventing a schema type. Each adapter's **pure** `build_request_body` fans this one field out to its provider wire form:

- **OpenAI-compatible** → top-level `response_format: { type: "json_schema", json_schema: { name, schema, strict } }`.
- **Anthropic** (no native `response_format`) → forced tool use: `tool_choice: { type: "tool", name }` + a single synthetic tool whose `input_schema` = the output schema. **Care**: Anthropic marks the *last* tool cacheable (`anthropic.rs:255-257`) — the synthetic schema-tool placement must not silently bust the deterministic cacheable tool prefix.
- **Gemini** → `generationConfig.responseMimeType: "application/json"` + `responseSchema`.

A per-provider **capability map** keyed off the `LlmProvider` enum chooses native-json-schema where supported, falls back to forced-tool-use otherwise, and treats in-engine validation as the **universal floor**. The `LlmProviderAdapter` trait and routing are **unchanged** ("add a provider = constructor + enum slot" stays true).

### 2. Validation: in the Rust engine, real JSON-Schema conformance (D2)

Validation runs **in-engine** on the deterministic, checkpointed path — not only in the SDK/control-plane (a worker-executed run must not emit unvalidated output). Introduce **one reviewed JSON-Schema crate** (candidate: `jsonschema`) for true nested/enum/format conformance — this dependency was itself owner-approved as part of D2. `extract_first_json` (`components.rs:379`) is promoted to a shared util for candidate extraction. A **byte-for-byte TS twin validator is deferred**: SDK execution is Rust-only (`RustEngineRequiredError`), so validation always runs in Rust; the TS path only needs to *accept and forward* the schema. (If the deprecated TS fallback engine must validate, that is a separate, flagged follow-up — see Deferred.)

### 3. Surface: an installable `structuredOutput` **middleware kind** (D4)

Structured output is exposed as a new **efficiency-layer middleware kind**, not a sealed governance control and not a bespoke node field:

```ts
agentNode("x", { llm, middleware: [{ kind: "structuredOutput",
  params: { name, schema, strict?, mode?: "required" | "lenient", retryCap?: number } }] })
```

It desugars into the existing single `resolvedMiddleware` list (exactly like `compress`/`terse`/`contextBudget`/`reflection`), so it **threads through the already-wired middleware path** — *not* the 14-hop node-config path. The bridge builds a `StructuredOutputMiddleware` from the spec, injected at a **pinned position** in the efficiency layer.

**Gate-safety holds by construction:** the approval gate is intrinsic to `MiddlewareStack::before_tool` (phase 3c), evaluated before any installed middleware, so a `structuredOutput` kind in the efficiency layer **cannot** route around it. `structuredOutput` is **not** in `GOVERNANCE_MIDDLEWARE_KINDS` (it is output-shaping, user-declarable), so the governance-rejection door does not fire on it.

`StructuredOutputMiddleware` over the existing hooks (`middleware.rs:76-144`):
- `before_model` sets `request.response_format` (and, for the Anthropic tier, the synthetic forced tool + a schema reminder).
- `after_model` extracts the candidate JSON (shared `extract_first_json`) and validates it; on invalid → `Err` to short-circuit fail-closed (the `RedactMiddleware` precedent).
- `after_run` attaches the parsed value into the new result field and records the validity verdict.

### 4. On-invalid: bounded deterministic retry → fail-closed; opt-in lenient (D1)

For `mode: "required"` (default): invalid JSON triggers a **bounded** re-loop within the existing `max_iterations` budget (proposed cap **2**), **no temperature drift**, re-prompting with the schema. If still invalid → typed `StructuredOutputInvalidError`, surfaced at the node sink as **channel data** (mirroring the `{ "error": "<msg>" }` gateway-error-as-data convention, `node.rs:79-87`) — **never a bare throw**. Retries are deterministic so replay/time-travel reach the same verdict, and validation is **idempotent across the approval re-run** (`runtime.rs:713-722`).

For `mode: "lenient"`: fail-**open** to raw text (the `ReflectionMiddleware` fail-open precedent) for advisory output. Routing a repeatedly-invalid output to a **human gate** stays available but opt-in (not the default).

### 5. Typed result field (additive)

Add `structured_output: Option<Value>` (+ a validity indicator) to the Rust `AgentResult` and `structuredOutput?` to the TS mirror, as additive, optional, camelCase, `skip_serializing_if = Option::is_none` fields exactly like `usage`/`todos`. The parsed value rides in the **same single** `NodeOutput::update` patch as the rest of the result (`node.rs:45-72`) to keep one-patch-one-checkpoint. The validity verdict, schema name, chosen strategy (native/forced/validate) and each retry are emitted as **audit events** ("Audit ⊇ traces", ADR 0028).

## Alternatives considered

- **Provider-native `json_schema` only (no fallback / no local validation)** — rejected: uneven across the OpenAI-compatible family; Anthropic has no `response_format`; `strict:true` is not a cross-provider guarantee → violates the mandatory-validation scope and breaks portability/determinism.
- **Forced-tool-use only** — rejected as sole strategy: works on any tool-capable provider but burns a tool turn, can collide with real tools, and is a weaker decoding constraint than native strict mode. Kept as the **fallback** tier.
- **SDK-side Zod validate+retry only** — rejected as the primary home: a Rust/worker-executed run would emit **unvalidated** output and the verdict would not be auditable engine state. The validate floor is kept, but it runs **in the Rust engine**.
- **Reuse the flat `jsonValidator` as the deep validator** — rejected for conformance: top-level type + flat `requiredKeys` only; no nested/enum/format. Usable as a shallow shape gate, not a substitute. (Owner chose the real crate, D2.)
- **A new bespoke `outputSchema` node field** (the design's original recommendation) — **not chosen**: the owner picked the middleware-kind surface (D4), which reuses the existing `resolvedMiddleware` threading and is gate-safe by construction, at the cost of a slightly less obvious authoring surface. The result field `structured_output?` is still added.
- **A sealed governance middleware** — rejected: it is output config, not a security control; it belongs in the user-declarable efficiency layer with a pinned position, not the sealed governed layer.
- **Parse the `final:` line out of `reasoning`** — rejected: fragile; the additive typed field is back-compatible and never breaks existing consumers.

## Consequences

- **Invariants preserved** — Determinism: retries bounded by `max_iterations`, no temperature drift, no wall-clock → stable replay. Checkpoint: result rides the same single patch → one-patch-one-checkpoint intact. Audit: verdict/schema/strategy/retries emitted as events. Gate: validation (`after_model`) runs before the sealed `before_tool` gate, which is intrinsic and cannot be bypassed by the efficiency-layer kind.
- **Additive / back-compat**: new optional camelCase serde-skip fields on `LlmRequest`/`AgentResult` and a new middleware kind; older specs and persisted graphs still deserialize via `#[serde(default)]`. Public-API change to engine packages (`LlmRequest`/`LlmResponse`, `AgentResult`) — **mandatory human review**; this ADR is that artifact.
- **Risk — provider divergence**: one neutral field fans out to three incompatible wire shapes plus capability gaps on local/aggregator providers; mitigated by the capability map + the validate-only floor.
- **Risk — response normalization**: structured data arrives as OpenAI content-string, Anthropic `tool_call.input` Value, or Gemini candidate text. A normalizing `structured: Option<Value>` on `LlmResponse` is **recommended** (8b) so callers need not know which provider they hit.
- **Risk — retry cost / non-determinism**: bounded re-loops add latency/tokens; the hard cap + fixed sampling keep replay stable.
- **Risk — Anthropic cache busting**: the synthetic schema-tool + `tool_choice` must respect the last-tool-cacheable breakpoint.
- **Deferred / honest gaps**: the TS twin validator (Rust-only execution makes it unnecessary now; flagged if the TS fallback must validate); the catalog `mapAgents` wire path is a known TODO (`run-catalog-graph.ts:249-251`) so persisted `mapAgents` nodes won't carry structured output until that path lands; per-provider strict guarantees documented per provider.

## Phasing

- **8a** — Gateway request wire field. `ResponseFormat::JsonSchema` + `LlmRequest.response_format` (`crates/llm-gateway/src/types.rs`), trait/routing unchanged. Mirror onto the TS `LLMRequest` for parity.
- **8b** — Per-adapter fan-out. OpenAI-compatible `response_format`; Anthropic `tool_choice` + synthetic schema-tool (cache-breakpoint care); Gemini `responseSchema`. Per-provider capability map. Optional normalizing `structured: Option<Value>` on `LlmResponse`.
- **8c** — Validation primitive. Promote `extract_first_json` to a shared util; add the reviewed JSON-Schema crate (D2).
- **8d** — `StructuredOutputMiddleware` over the middleware hooks; bounded retry within `max_iterations`, no temperature drift; idempotent across approval re-run.
- **8e** — Typed result field. `structured_output: Option<Value>` (+ validity) on Rust `AgentResult`; `structuredOutput?` on TS mirror. Sink in the same single patch; failure surfaces as channel data, never a throw.
- **8f** — Surface wiring (middleware-kind path). New `structuredOutput` `EfficiencyMiddlewareSpec` kind (SDK `agent-node.ts`), desugar into `resolvedMiddleware`, build arm in `bridge.rs build_agent_middleware`, `MiddlewareSpec` round-trip in `spec.rs`, contracts efficiency union. **Not** in `GOVERNANCE_MIDDLEWARE_KINDS`.
- **8g** — Audit events (verdict/schema/strategy/retries) + docs (`docs-site/docs/advanced-agents/middleware-and-profiles`, per-provider guarantees). Position `StructuredOutputMiddleware` relative to the sealed governed hooks.
- **8h** (deferred, flagged) — TS fallback engine validation + catalog `mapAgents` carrier, only if needed.
