# Changelog

All notable changes to the Adriane engine are documented here. The project follows
[Semantic Versioning](https://semver.org/).

## 1.18.1

### Fixed

- **`answerBuilder` / `outputParser` / any component reading an agent's output channel now unwrap the
  `AgentResult`.** `value_to_text` treated an `AgentResult` object as raw JSON, so a downstream
  component received the stringified wrapper (`{ reasoning: "thought:…\nfinal:…", approvalRequests, … }`)
  instead of the agent's answer. It now extracts the final answer — the validated `structuredOutput`
  when present, else the text after the last `final:` marker in `reasoning`. Fixes Governed Ask
  answers, co-authoring projections, and the enterprise-analysis proposal all surfacing as raw JSON.

## 1.18.0

### Changed

- **Council is now a native catalog graph.** `council(...)` returns a `GraphDefinition` whose anonymize/aggregate steps are Rust catalog components (`councilAnonymize` / `councilAggregate`) instead of JS handlers — so a council runs on the Rust engine via `runCatalogGraph`, like every other governed graph (dogfood / Rust-only, ADR 0003). BREAKING vs 1.17.0: `council(...)` returns a `GraphDefinition` (run it with `runCatalogGraph(council(...))`), not a `CompiledGraph`. The pure `anonymizeAndShuffle` / `aggregateRanks` helpers stay exported. (ADR 0061)

## 1.17.0

### Added

- **LLM Council** — `council({ members, reviewers?, chair, humanGate? })` builds a governed
  deliberation graph: dispatch → members (fan-out) → anonymized peer-review (fan-out) → Borda
  aggregate → optional human gate → chair synthesis. Native agent seats (a member never reviews its
  own answer; every seat audited), deterministic replay-faithful anonymize + aggregate. (ADR 0013/0061)

## 1.16.0

### Added

- **Cross-encoder reranking (ADR 0060 E1)** — a `reranker` node now re-scores its candidates through a
  real cross-encoder (`BAAI/bge-reranker-v2-m3`) served by a self-hostable, EU-sovereign rerank service
  (HuggingFace TEI), configured by `ADRIANE_RERANK_ENDPOINT`. The gateway holds the HTTP call behind a
  transport seam; the runtime routes `reranker` nodes to it. **Graceful fallback**: with no endpoint the
  reranker is an identity passthrough that preserves the upstream ranking (no external call, no
  mock-cosine rescoring). Fail-open: a rerank error keeps the upstream order.

## 1.15.0

### Added

- **Per-token streaming on the catalog run path** — `runCatalogGraph({ streamTokens: true })` surfaces
  an agent node's generation as `token_delta` run events over `onEvent`, so a catalog run (e.g. the
  product's Governed Ask) can stream its answer token-by-token instead of only returning a final
  result. Reuses the streaming chain already wired for the in-process builder path (`CompiledGraph.stream`)
  — no Rust or napi change; the assembled state is byte-identical (deltas are observational, they bypass
  the checkpoint/journal). Default off. (ADR 0033, ADR 0060)

## 1.14.0

### Added

- **Dynamic `mapAgents` fan-out on the catalog path** — a `mapAgents` carrier lets a catalog graph fan a
  node out over a runtime-sized list, executed natively; a malformed carrier now warns instead of failing
  silently. (ADR 0049)

## 1.0.0

First stable engine release. The Rust runtime reaches (and extends) parity with the
TypeScript runtime, gains durable timers and external signals, and the knowledge layer
(OKF + KB/KG) descends into the engine.

### Added

- **Concurrent, deterministic fan-out** — a node's branches run in parallel off a shared
  pre-fan-out snapshot and merge in declared order (fixes the prior sequential port that
  accumulated state between branches). (ADR 0008)
- **Subgraphs** — a `subgraph` node runs a registered child graph (sharing the runtime's
  registries / checkpointer / event bus), maps channels in and out, and propagates child
  suspension; a child suspended on an internal human gate resumes across napi calls. (ADR 0008)
- **Incremental streaming** — `CompiledGraph.stream()` projects all four modes
  (`values` / `updates` / `messages` / `debug`) incrementally on the Rust engine. (ADR 0008)
- **Durable timers + external signals** — `NodeOutput.sleep_until` / `wait_for_signal`,
  `GraphRuntime::resume_with_signal`, napi `engine_signal`, and SDK
  `sleepUntil` / `waitForSignal` / `CompiledGraph.signal` / `readSuspendMeta`. Two new
  generalized suspend reasons; the engine stays clock-free (`wakeAt` is data — the control
  plane schedules the wake). (ADR 0009)
- **Dynamic-message `send` / inbox** — pre-queue per-node inputs (`RunOptions.inbox`),
  each consumed one-per-execution via the reserved `__injected` channel: the map-reduce seam.
- **`@adriane-ai/okf` + `adriane-okf`** — the Open Knowledge Format parser/serializer
  descends into the engine (byte-compatible TypeScript + Rust, no YAML/regex dependency).
- **`@adriane-ai/knowledge` + `adriane-knowledge`** — the knowledge-base + knowledge-graph
  model, pure graph ops (build-graph, depth-limited neighbors, cosine search), and the
  `KnowledgeStore` seam (+ an in-memory implementation).

### Changed

- **`RunEvent` wire fields are now camelCase** (`runId` / `nodeId`, was snake_case) to match
  the TypeScript `RunEvent` the SDK parses — `event.nodeId` was `undefined` on the JS side.
  Consumers reading `run_id` / `node_id` off a forwarded event must switch to `runId` / `nodeId`.

## 0.2.0

Additive, backward-compatible engine features.

### Added

- **Multi-provider LLM gateway** — a **native Google Gemini** adapter (`generateContent`)
  plus the OpenAI-compatible family: **OpenAI, OpenRouter, MiniMax, Hugging Face, LM Studio**
  alongside the existing Mistral and Ollama. A new provider is an enum slot + a constructor;
  selection is by which env credential is present, so a deployment brings its own model
  (BYOM) and can run fully on-premise with local models. (ADR 0005, #24)
- **`semanticRetriever` component** — genuine semantic retrieval: ranks pre-embedded chunks
  by cosine similarity to a pre-embedded query (real embeddings, e.g. Mistral), unlike the
  mock-embedding `retriever`. (#25)
- **Knowledge base as MCP resources** — the MCP server exposes a knowledge base as MCP
  `resources` (`resources/list` + `resources/read`), so any MCP client (Claude Desktop, an
  IDE, another agent) can read it through the open standard. (#26)
- **Contracts** — knowledge, compliance, and LLM-router DTOs added to `@adriane-ai/contracts`. (#26, #27)
- **ADR 0006** — sovereign deployment modes (EU cloud / private cloud / true on-premise) and
  granular per-knowledge-base permissions. (#27)

### Notes

- The deprecated TypeScript fallback gateway intentionally stays at two adapters; the
  broader provider family lives on the Rust engine (the default execution path).

## 0.1.0

Initial public release: the Rust agentic graph runtime, the TypeScript & Python SDKs over
it, the Adriane DSL compilers, the component/agent library, the CLI, and the MCP plugin.
