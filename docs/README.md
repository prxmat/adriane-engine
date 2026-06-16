# Adriane documentation

Adriane is a **stateful, resumable agent-graph runtime** with a Rust engine and a TypeScript
SDK. You build a graph of nodes (actions, agents, tools, human-approval gates), compile it,
and run it. Every run is **deterministic by default**, **checkpointed after every node**, and
**resumable from where it stopped** — including across process restarts and human approvals.

> **Pre-release.** Adriane is not published to npm/PyPI yet. You install it **from source**
> in this monorepo. The public surface is `@adriane/graph-sdk` (TypeScript) and the
> `adriane` Python package over the same Rust engine. Everything in this doc is anchored to
> the real, shipped API.

## Where to start

1. **[Getting started](./getting-started.md)** — install from source and get a first run
   green in under 10 minutes.
2. Then work through the tutorials below in order. Each one is **offline** (a deterministic
   mock LLM, no API key) and maps to a runnable, self-verifying example in
   `packages/graph-sdk/examples/`.

## Tutorials

| # | Tutorial | What you'll learn |
| --- | --- | --- |
| 01 | [Your first graph](./tutorials/01-your-first-graph.md) | `createGraph`, channels, nodes, edges, `run()` |
| 02 | [Agent nodes](./tutorials/02-agent-nodes.md) | `agentNode`, ReAct loop, `AgentResult`, prebuilt micro-agents |
| 03 | [Tools and tool nodes](./tutorials/03-tools-and-tool-nodes.md) | `InMemoryToolRegistry`, agent tool-calling, `toolNode`, components |
| 04 | [Human-approval gates](./tutorials/04-human-approval-gates.md) | `humanGate`, `suspendForApproval`, `approveAndResume` |
| 05 | [Checkpointing and resume](./tutorials/05-checkpointing-and-resume.md) | The checkpoint contract, suspend/resume, run ids |
| 06 | [Streaming](./tutorials/06-streaming.md) | `stream()`, `onEvent()`, `streamAgentTokens()` |
| 07 | [Python SDK](./tutorials/07-python-sdk.md) | `validate_graph`, `compile_graph_yaml`, `run_component`, `run_prebuilt` |
| 08 | [The Adriane DSL](./tutorials/08-the-adriane-dsl.md) | Authoring graphs as YAML, validating and compiling them |

## Reference

Beyond the tutorials, two reference docs go deeper:

- **[Architecture](./architecture.md)** — the engine layers, the dependency rule, and the
  Rust ↔ TypeScript napi bridge.
- **[CLI](./cli.md)** — the full `adriane` command reference (validate / compile / run /
  publish / diff / init).

## The public surface at a glance

Everything you need for the common case is re-exported from a single package:

```ts
import {
  createGraph,        // start a graph builder
  components,         // 30 pure compute components (promptBuilder, retriever, ...)
  prebuilt,           // 16 prebuilt micro-agents (summarizer, classifier, ...)
  semanticRetriever,  // vector-store retrieval node
  createEmbeddings,   // text embeddings client
  createVectorStore,  // in-memory / persistable vector store
  rustEngineAvailable // is the native Rust engine loaded?
} from "@adriane/graph-sdk";
```

The builder is fluent and fully typed — declared channels accumulate into the state type, so
`run()` / `resume()` return a typed result with no manual annotation.

## How execution works

- Execution runs on the **Rust engine** (via the `@adriane/napi` native addon) when it is
  present, otherwise it **falls back to the bundled TypeScript engine**. The public API
  (`run` / `resume` / `approveAndResume` / `stream` / `onEvent`) is identical either way.
- Check which engine you are on with the `usesRustEngine` property on a compiled graph, or
  the `rustEngineAvailable()` helper.
- Conditional routing is **always a named predicate you register** — never an `eval`'d
  string. This is a non-negotiable safety invariant.

## Conventions in these docs

- Code blocks are **real API**. If something is not proven by the shipped code it is marked
  `TODO` rather than invented.
- Every TypeScript example is offline by default — agent nodes get a `DefaultLLMGateway` with
  a `MockLLMProviderAdapter`, so you can run everything with no keys.
- Build the native addon (`pnpm napi:build`) to run on the Rust engine; without it the SDK
  prints one warning per process and runs on the TypeScript engine.

## License

The engine is Apache-2.0. See the root [`LICENSE`](../LICENSE) and [`NOTICE`](../NOTICE).
