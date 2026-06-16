# Adriane — Rust engine (migration in progress)

This workspace is the staged Rust port of the open-source engine (the TypeScript
`packages/*`). It exists alongside the TS engine, not as a big-bang replacement —
each crate lands compiling and tested before the next, and the TS engine keeps
working throughout. See `docs/adr/0002-migrate-engine-to-rust.md` for the plan and
rationale.

## Status

Verified with `cargo 1.96.0` (fmt + clippy `-D warnings` + `cargo test --locked`):
**349 tests green** across 18 crates, plus a Node smoke test through the napi bindings, the `adriane` CLI binary, an
MCP server (Claude Code plugin), and a Python SDK (pyo3) — three consumers of the one
Rust engine. The TS SDK's `safeCompile()` already calls the Rust validator
when the addon is present (graceful TS fallback otherwise).

| Crate | Mirrors | Status |
| --- | --- | --- |
| `adriane-graph-core` | `@adriane/graph-core` | ✅ **Done** — ids, types, errors, `validate_graph`. 8 tests. |
| `adriane-graph-runtime` | `@adriane/graph-runtime` | ✅ **Async, near-parity** — async node handlers (tokio), start/resume/suspend, default + conditional edges, reducers, checkpointer, event bus, DynamicInterrupt + `update_state`, fan-out → join, recursion limit, **retries + `run_failed`, time-travel `replay_from` (fork), live event observers**. 13 tests. Deferred: streaming modes, subgraphs. |
| `adriane-approval-engine` | `@adriane/approval-engine` | ✅ **Done** — engine (no self-approval) + Ed25519 attestation (canonicalise / sign / verify / chain). 9 tests. |
| `adriane-llm-gateway` | `@adriane/llm-gateway` | ✅ **Gateway + real Anthropic adapter** — async `LlmGateway` / `LlmProviderAdapter`, `DefaultLlmGateway`, `MockAdapter`, **`AnthropicAdapter` (reqwest/rustls, testable port seam, cache-aware prefix, tool_use → tool_calls)**. 14 tests. Deferred: structured content blocks, streaming. |
| `adriane-agents-core` | `@adriane/agents-core` | ✅ **ReAct + patterns** — ReAct loop (native tool calls + `FINAL:`/`ACTION:`), no-self-approval tool registry, `agent_node_handler` (suspend-for-approval via `__approvedTools`), **plan-execute, reflection, supervisor, working-memory**. 26 tests. Deferred: swarm handoff, scratchpad. |
| `adriane-graph-adriane` | `@adriane/graph-adriane` | ✅ **DSL compiler** — YAML → AST → DSL validation → transform → structural gate; byte-equivalent JSON to the TS compiler. 14 tests. |
| `adriane-lang-adriane` | `@adriane/lang-adriane` | ✅ **Prompt/agent/chain DSL compiler** — same pipeline as graph-adriane, byte-equivalent JSON to the TS compiler. 21 tests. |
| `adriane-memory-store` | `@adriane/memory-store` | ✅ **BaseStore + InMemoryStore** — namespaced get/put/delete/search/list, insertion-ordered. 6 tests. PgStore deferred. |
| `adriane-artifact-store` | `@adriane/artifact-store` | ✅ **Versioned store** — `Artifact`/`ArtifactRef`, write increments version, read/list. 12 tests. PgArtifactStore deferred. |
| `adriane-callbacks` | `@adriane/callbacks` | ✅ **CallbackManager** — 15-variant `CallbackEvent`, async dispatch with handler isolation, child managers, built-in handlers. 11 tests. |
| `adriane-observability` | `@adriane/observability` | ✅ **Tracer + metrics + bus** — spans (start/child/end + duration), counter/gauge/histogram, event bus fan-out/unsubscribe. 11 tests. |
| `adriane-runnable` | `@adriane/runnable` | ✅ **Composable Runnable** — async `Runnable<I,O>`, lambda/passthrough/sequence (`then`)/parallel (join_all, keyed). 9 tests. Fluent `.pipe()` → explicit constructors (documented). |
| `adriane-config` | `@adriane/config` | ✅ **Env + feature flags** — pure `parse_env(map)` (aggregate validation errors) + `get_env()` (OnceLock), typed flags. 13 tests. |
| `adriane-rag-pipeline` | `@adriane/rag-pipeline` | ✅ **RAG (mock/in-memory)** — Embedder/Splitter/VectorStore/Retriever/Reranker seams, deterministic MockEmbedder, cosine top-k, end-to-end pipeline. 23 tests. Deferred: real embeddings/loaders. |
| `adriane-napi` | (bindings) | ✅ **Boundary live** — `validateGraphJson`, `compileGraphYamlJson`, `engineVersion` callable from Node; the SDK's `safeCompile()` uses it. See `bindings/README.md`. |
| `adriane-cli` | `@adriane/adriane-cli` | ✅ **CLI binary** — `adriane compile/validate/run/inspect/--help`; run drives the async runtime (suspends cleanly at human gates), event journal to stderr. 13 tests. |
| `adriane-py` | (Python SDK) | ✅ **2nd SDK (pyo3)** — `python/adriane` wraps the Rust core (`validate_graph`, `compile_graph_yaml`, `engine_version`); abi3, builds with cargo. Proves multi-language SDKs over one engine. 5 py tests. |
| Claude Code plugin | `plugin/` | ✅ **Plugin** — skill (authoring graphs), slash commands (`/adriane:compile|validate|run|new` over the CLI), MCP server exposing the Rust engine (`validate_graph`, `compile_graph_yaml` via `@adriane/napi`). MCP smoke verified. |
| `graph-sdk` · `contracts` | (TS-facing surfaces) | Stay TS (SDK facade over napi; contracts is the API↔Studio DTO boundary). |

## Build & test

```bash
# Install rustup if needed:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# From the repository root. rust-toolchain.toml pins the exact Rust version.
pnpm rust:check     # fmt + clippy -D warnings + tests
pnpm rust:build     # compile the full Rust workspace
pnpm rust:fmt       # formatting only
pnpm rust:lint      # clippy only
pnpm rust:test      # tests only
```

## Design notes

- **Wire-compatible:** types serialize to the same camelCase JSON as the TS model
  (`type`, `entryNodeId`, `retryPolicy`, `human-gate`, …), so a Rust engine and the
  TS control plane / SDK can exchange definitions and state during the migration.
- **`#![forbid(unsafe_code)]`** in every crate.
- Validation returns a `Vec<ValidationError>` (every problem at once), matching
  `validateGraph`.
