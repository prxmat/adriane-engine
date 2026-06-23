# ADR 0030 — Multimodal content (deep-agent phase 9)

- **Status**: Accepted (design signed off 2026-06-23; key decisions D1–D5 confirmed by the project owner)
- **Context**: deep-agent harness phase 9 of [ADR 0023](0023-governed-deep-agent-platform-landscape.md)
- **Relates to**: the additive / opt-in / gate-safe spine of [ADR 0029](0029-governed-structured-output.md); the artifact store ([ADR 0024](0024-governed-virtual-filesystem-seam.md) backend).

## Context

Phase 9 of ADR 0023 mandates: *"`LlmMessage.content` is text-only. Move it to content blocks (text | image), mapped per adapter — images in, at least."* Today the engine message type is text-only: `crates/llm-gateway/src/types.rs:26` declares `content: String`; the sole constructor `LlmMessage::text` sets it from a string; `react.rs:11-13` documents the gap in code.

Grounded facts that shape the design:
- The change lives **entirely in `llm-gateway`'s `LlmMessage`/`LlmResponse`** — Rust `graph-core` has **no** `Message`/`ContentBlock` concept (the only `ContentBlock` is an Anthropic *response* wire struct).
- **Blast radius** of changing `content`'s type: ~10 wire-consumption reads (anthropic 204/217/234/317, gemini 162/193/201/208, openai 347, redactor 152/176) + 22 `::text` callers + 2 struct literals in react.rs (283-289, 467-473).
- **Anthropic already speaks content blocks** on the wire (`AnthropicMessage.content` is `serde_json::Value`) — the natural model + cheapest fan-out point.
- On the Rust path the agent input is hardcoded `Value::Null` (`node.rs:57`); the seed user message is built text-only from `Input/State` (`react.rs:199-202`). So an image must originate from the **run input** or a channel.
- Each checkpoint stores the whole `GraphState` and each `node_completed` event carries the output patch — **inline image bytes would re-serialize into every checkpoint and event**.
- The PII redactor scrubs only flat `content` (redactor.rs:144-188) — block text would bypass it; image PII is invisible to a text redactor.

This ADR is the mandatory-human-review artifact for a public-API change to an engine package (`LlmMessage`/`LlmResponse`).

## Decision

Additive, opt-in, back-compatible — the ADR-0029 spine, never a structural rewrite. Owner choices (D1–D5):

### 1. Message shape — additive `content_blocks` (D2)
Keep `content: String` **exactly** and add one optional field on `LlmMessage` (and, for output, `LlmResponse`):

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub content_blocks: Option<Vec<ContentBlock>>,

#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Image { source: MediaSource },
    Audio { source: MediaSource },
    File  { source: MediaSource },
}
```

Same additive pattern as `tool_calls`/`tool_call_id`/`tool_name`. A sibling `LlmMessage::with_blocks(role, blocks)` constructor leaves all 22 `::text` callers untouched. Omitted on the wire when `None`, so every existing payload / persisted checkpoint / `::text` call stays valid with **zero edits** and text-only requests serialize byte-identically.

### 2. Media source — ArtifactRef ref-not-bytes, capped inline escape hatch (D1, D5)
```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MediaSource {
    Artifact { artifact_id: String, version: Option<u32>, media_type: String }, // default (ref)
    Url      { url: String, media_type: Option<String> },                       // stable / content-addressed
    Base64   { media_type: String, data: String },                              // inline, size-capped
}
```
**Default = ArtifactRef** (a small, stable pointer in the channel/checkpoint), resolved to bytes **just-in-time at the gateway boundary** by a `MediaResolver` seam — so `GraphState`/checkpoints/events stay small and replay-stable. Inline `Base64` is allowed only **size-capped** (spill to the artifact store above a threshold); unbounded inline is rejected. `Url` must be **stable / content-addressed** (a volatile signed URL would break replay-stability, since the block's serde form is part of the replayed conversation).

### 3. Per-adapter fan-out, gated on `content_blocks.is_some()` (in)
Adapters stay pure; the `MediaResolver` resolves `Artifact` sources to `Base64`/`Url` **before** dispatch, so each adapter only formats `Base64`/`Url`. Text-only requests serialize byte-identically (existing exact-shape unit tests are the regression anchor). Per provider:
- **Anthropic** — branch the plain-message arm to emit `[{type:text}, {type:image, source:{type:base64, media_type, data}}]` (reuses the existing `Vec<Value>` block builder).
- **OpenAI-compatible** — switch `content` from string to `[{type:text}, {type:image_url, image_url:{url:"data:<mime>;base64,…"|<url>}}]`.
- **Gemini** — append `{inlineData:{mimeType,data}}` / `{fileData}` parts.
A **vision/modality capability gate** (the ADR 0029 capability-map pattern) rejects an image/audio/file request on a provider/model that cannot accept it, rather than silently dropping it. System/tool turns stay text-only.

### 4. Scope — full multimodal in + out, honestly bounded (D4)
**Input**: image + audio + file blocks on user messages, all three adapters. **Output**: `LlmResponse.content_blocks` (additive) populated where the **chat** API returns inline media (chiefly Gemini inline images); **OpenAI/Anthropic chat do not generate media** — image/audio *generation* is a separate provider API and is a named **future seam**, not wired through the chat adapters here. Audio/file *input* ships; audio/file *output* rides the same `content_blocks` mechanism if/when a chat API returns it.

### 5. Entry surface — a dedicated multimodal input on the run/EngineSpec (D3)
Add an explicit multimodal-input field on `EngineSpec`/`AgentSpec` (threaded SDK → wire → bridge → handler), so `react.rs:199-202` emits image/audio/file **content blocks** from it instead of JSON-stringifying a channel map (which would also burn tokens). This avoids guessing which channel is binary and keeps `initialData` as the text input path.

### 6. Redaction / governance
Extend `redactor.rs` to also collect + rewrite **text** blocks inside `content_blocks` (so block text is not a redaction bypass). **Image/audio/file PII** is invisible to the text redactor — policy = documented pass-through gap with an optional fail-closed-on-media switch and a future media-redaction seam. An `Artifact`-sourced block reuses the versioned/immutable, governable artifact store. `CompressMiddleware`/`ContextBudgetMiddleware` operate on `content: String` only — documented coverage gap for block text.

## Alternatives considered
- **Breaking `content: String → enum Content { Text | Blocks }`** — rejected: rewrites all 24 construction sites + 10 reads + redactor + middleware + TS mirror + napi, and risks deserialization failure on persisted checkpoints. Violates ADR 0029 "never a structural rewrite."
- **Separate multimodal message path / vision node** — rejected: forks the agent loop into two message types and diverges from the industry content-block convention the TS side already uses.
- **Inline base64 everywhere** — rejected as default: a 2 MB image re-serializes into every checkpoint and event. Kept only as a capped escape hatch.
- **Image via a `before_run` middleware** — rejected as primary: middleware runs after the text seed is built, so it cannot retrofit blocks until the message *type* supports them.

## Consequences
- **Determinism**: preserved only if blocks carry stable bytes or a content-addressed / artifact ref; a volatile URL or timestamp in a block breaks replay.
- **Checkpoint size**: the ref-not-bytes default keeps state small; the inline escape hatch needs a hard size cap.
- **Back-compat by construction**: `content` stays `String`, the new fields are `serde(default, skip_serializing_if)`, so older specs / checkpoints deserialize and text-only requests are byte-identical.
- **Redaction gap** (security-relevant): block text is now scrubbed; media PII is a documented gap / fail-closed option.
- **Capability gating**: non-vision providers reject rather than silently drop.
- **Wire parity**: the block variants must exist on **both** the Rust `ContentBlock` and the TS `LLMContentBlock` union with matching tags/casing for true round-trip.

## Phasing & delivery status

This is a large feature; it lands as a **foundation increment** first, then focused follow-ups on the stable base.

**Shipped in the foundation PR:**
- **9a** ✅ Types (Rust): `ContentBlock` (text/image/audio/file) + `MediaSource` (artifact/url/base64) enums; additive `content_blocks` on `LlmMessage` **and** `LlmResponse`; `with_blocks` constructor; `content: String` kept; text-only serializes byte-identically (tested). Wire field casing: tags `snake_case`, fields `camelCase` (TS parity).
- **9b** ✅ Per-adapter input fan-out (anthropic / openai / gemini), gated on `content_blocks.is_some()` so the text path is byte-identical; image (base64 + url) tested per adapter; audio/file mapped where each provider supports it (OpenAI `input_audio`/`file`, Gemini `inlineData`, Anthropic `document`).
- **9d** ✅ Redaction: the PII redactor now scrubs **text blocks** inside `content_blocks` (closing the bypass); media-PII is a documented text-redactor gap. Compress/context-budget operate on `content` only — documented coverage gap.

**Deferred (named follow-ups on this base):**
- **9c** ⏳ `MediaResolver` seam — `Base64`/`Url` work end-to-end today; `Artifact` resolution (gateway → artifact-store, resolve to bytes at the boundary) is the heaviest hop and lands next.
- **9e** ⏳ Entry surface — a multimodal input field on EngineSpec/AgentSpec → bridge → handler → `react.rs` seed emits blocks. Until this lands, the gateway can carry/send multimodal messages but a run has no first-class way to attach media (any code building `LlmMessage::with_blocks` already gets correct multi-provider behaviour).
- **9f** ⏳ TS parity — the `LLMContentBlock` image/audio/file variants + the deprecated TS adapters' fan-out land with 9e (kept out of this PR so it does not touch the dead TS engine; the engine path is Rust, and the wire is JSON).
- **9out** ⏳ Output — parse `LlmResponse.content_blocks` where the chat API returns inline media (Gemini). Media *generation* (OpenAI/Anthropic separate APIs) = named future seam.
