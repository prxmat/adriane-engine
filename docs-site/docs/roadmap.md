---
sidebar_position: 13
title: Roadmap
description: Honest feature status (stable / experimental / reserved) and where Adriane is going.
---

# Roadmap

Adriane is **0.2.0 — alpha**. This page is the honest ledger: what you can rely on today, what
exists but isn't proven, what is reserved in the schema but **not implemented**, and where the
project is headed. We'd rather under-promise here than have you discover a gap in production.

:::warning Alpha — read before you build on it
Treat anything not marked **Stable** as subject to change or absence. In particular, the
**Reserved** rows below have slots in the type system but **no runtime behaviour** — do not
design around them yet.
:::

## Feature status

Legend: **Stable** = relied on, contract-tested · **Experimental** = works, surface may change ·
**Reserved** = schema slot exists, **not implemented in the runtime**.

| Capability | Status | Notes |
| --- | --- | --- |
| Deterministic execution (named-predicate routing) | Stable | Conditions are names resolved in the `ConditionRegistry`, never `eval`'d. See [execution contract](/docs/core-concepts/execution-contract). |
| Checkpoint after every node + state mutation | Stable | The engine ships the `Checkpointer` interface + `InMemoryCheckpointer`. Durable Postgres checkpointing is **Adriane Studio** (commercial), not an open-SDK export. |
| Lifecycle events (`node_started` … `run_completed`/`run_failed`) | Stable | The event journal is the audit trail. |
| Suspend / resume + human gates | Stable | `run_suspended` / `run_resumed`; resume re-validates the checkpoint with Zod. |
| Governance: approval gates, separation of duties, Ed25519 attestation | Stable | The Rust engine enforces no-self-approval + attestation and emits lifecycle events; the SDK exposes the approval API (`humanGate` / `suspendForApproval` / `approveAndResume` / `onEvent`). Binding approvals to authenticated principals, persisting the audit journal, and the live view are a control-plane concern (**Adriane Studio**, or one you build on the SDK). See [governance](/docs/governance/governance-model). |
| Recursion limit | Stable | `RecursionLimitError` bounds cyclic runs. |
| One Rust engine + TypeScript SDK (`@adriane-ai/graph-sdk`) | Stable | The Rust engine (`@adriane-ai/napi`) is a **required** dependency. |
| TypeScript engine path (dev/test/uncovered platforms) | Stable | Not deprecated — it's the fallback when the native addon is absent. |
| Python SDK (`pip install adriane-ai` → `import adriane_ai`) | Experimental | JSON-in/JSON-out: validate, compile, model policy, component & prebuilt runs. **No custom Python nodes, no streaming** — by design. See [one engine, two languages](/docs/sdk-parity/one-engine-two-languages). |
| Adriane DSL (compile graph/agent/chain YAML) | Experimental | Compiles in both SDKs from the same Rust compiler. |
| Multi-provider LLM gateway (Anthropic, Gemini, OpenAI-compatible family, local) | Stable | Native Anthropic & Gemini + OpenAI-compatible OpenAI/OpenRouter/MiniMax/Hugging&nbsp;Face/Mistral + local Ollama/LM&nbsp;Studio; env-selected (BYOM). New in 0.2.0. See [Providers](/docs/building/providers). |
| Semantic retrieval (`semanticRetriever` component) | Experimental | Real-embedding cosine retrieval over a supplied corpus + query vector (vs the mock-embedding `retriever`). New in 0.2.0. |
| MCP server (tools + knowledge-base resources) | Experimental | Run agents/graphs as MCP tools and read a knowledge base as MCP resources, over stdio. New in 0.2.0. See [MCP server](/docs/building/mcp-server). |
| Streaming runs | Experimental | TypeScript only today. |
| Rust incremental streaming | Reserved | The Rust path does not yet stream incrementally; streaming is a TS-SDK capability for now. |
| Parallel fan-out (`NodeDefinition.fanOut`) | Reserved | Schema slot only — **not executed** by the runtime. |
| Subgraph execution (`NodeDefinition.subgraphId`) | Reserved | Schema slot only — **not executed** by the runtime. |
| Durable timers / signals | Planned | Not present in the engine. See below. |
| Control plane, worker fleet & governance UI | Adriane Studio (commercial) | The control-plane API, the BullMQ worker fleet, durable Postgres checkpointing, and the governance Studio UI are **Adriane Studio**, the managed platform — not part of this open engine repo. The engine is a library you embed; there is no server to run for the engine itself. |
| Polyglot SDKs beyond TS/Python (Go, Java, PHP, .NET, Ruby, native Rust) | Planned | The architecture is built for this; none are shipped. See below. |

:::note Reserved means absent
`fanOut` and `subgraphId` appear in `NodeDefinition` and the architecture
[overview](/docs/architecture/overview) calls them out explicitly: the slots are there so the
data model is stable, but **the runtime does not act on them**. A graph that sets them will not
fan out or descend into a subgraph today.
:::

## The vision

The bet behind Adriane is a single Rust core with **thin language bindings**, so the same engine
— same validator, same DSL compiler, same governance — can be driven from many languages without
a second implementation to drift.

### Polyglot SDKs

This is already real for two languages and is the clearest path forward:

```mermaid
flowchart TB
  core["Rust engine core<br/>(graph model · validator · DSL compiler · governance)"]
  napi["napi binding"] --> ts["TypeScript SDK ✅ shipped"]
  pyo3["pyo3 binding"] --> py["Python SDK 🟡 experimental"]
  future["future thin bindings"] --> rest["Go · Java · PHP · .NET · Ruby · native Rust SDK<br/>📋 planned"]
  core --> napi
  core --> pyo3
  core --> future
```

The TypeScript SDK rides a [napi](/docs/architecture/napi-bridge) binding; the Python SDK rides a
pyo3 binding. The same pattern — a thin, JSON-shaped binding over the Rust core — is what makes
Go, Java, PHP, .NET, Ruby, and a native Rust SDK *tractable* rather than rewrites. They are
**planned, not shipped**; the design exists to make them additive.

### Durable timers and signals

Today a run suspends at a human gate and resumes from a checkpoint (via the `Checkpointer`
interface — `InMemoryCheckpointer` in-process, or your own/Adriane Studio for durability). The
next step is time-and-event-driven durability: **durable timers** (resume after a delay that
survives process restarts) and **signals** (resume on an external event), so a governed run can
wait on the real world without holding a process. This moves Adriane toward the durability
properties that tools like Temporal are known for — see
[the comparison](/docs/introduction/comparison) for the honest gap today.

### The managed platform: Adriane Studio

The engine in this repo is a **library you embed** — there is no server to run for the engine
itself. Like Temporal separates the open SDK from the Temporal Service/Cloud, Adriane separates
this open engine from **Adriane Studio**, the managed control plane: durable Postgres
checkpointing, a scalable worker fleet (self-registration, heartbeating, graceful drain), and the
governance Studio UI (live view, audit journal, approvals bound to authenticated principals).
Studio's code is **not** in this repo. If you'd rather build your own control plane, the SDK gives
you everything you need: the `Checkpointer` interface, the approval API (`humanGate` /
`suspendForApproval` / `approveAndResume`), and `onEvent` for the lifecycle stream.

### More integrations

A larger, curated catalog of **LLM providers** (via the gateway — the only layer allowed to
import provider SDKs) and **vector stores / retrievers** for the RAG pipeline. Breadth here is
deliberately behind [Haystack](/docs/introduction/comparison) today; closing some of that gap is
on the path, scoped to what stays governable and deterministic.

## How to read this page over time

Rows move **left to right** — Reserved → Experimental → Stable — only when there's runtime
behaviour and a contract test behind them. If a capability you need is Planned or Reserved, it is
not there yet, full stop. When in doubt, the [execution contract](/docs/core-concepts/execution-contract)
and the [architecture overview](/docs/architecture/overview) are the source of truth for what the
runtime actually does.

## See also

- [How Adriane compares](/docs/introduction/comparison) — vs LangGraph, Temporal, Haystack.
- [Architecture overview](/docs/architecture/overview) — including the reserved `fanOut`/`subgraph` slots.
- [One engine, two languages](/docs/sdk-parity/one-engine-two-languages) — the parity contract.
