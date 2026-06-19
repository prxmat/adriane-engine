---
sidebar_position: 1
title: Why Adriane
description: What Adriane is, the problem it solves, and the mental model behind it.
---

# Why Adriane

Adriane is a framework for building **agentic graphs**: stateful workflows where LLM
agents, tools, and humans collaborate over a shared, typed state. It is built for the
part most agent frameworks treat as an afterthought — **what happens when a run has to
pause, be approved, fail, or be replayed exactly.**

## The problem

An agent that calls a few tools in a loop is easy to write and hard to operate. The
moment it touches anything real — issuing a refund, deploying code, sending an email —
you need answers to questions a `while` loop can't give you:

- **Can it pause for a human, then resume from exactly where it stopped?**
- **Who approved this action, and can the agent approve its own work?** (It must not.)
- **If it crashes mid-run, can it resume without re-doing — or re-charging — completed steps?**
- **What happened, in what order, and can I replay it to find out?**

Adriane makes these first-class. A graph is **deterministic by default**, **checkpointed
after every step**, **observable through a lifecycle event for every transition**, and
**governed** through human-approval gates that suspend the run cleanly and resume it from
the latest checkpoint.

## What you get

| Guarantee | What it means in practice |
| --- | --- |
| **Deterministic** | The same graph + same inputs → the same execution. Conditions are *named predicates*, never `eval`'d strings. |
| **Resumable** | Every node completion and state mutation is checkpointed. A crashed or suspended run resumes from the latest checkpoint — no re-running completed work. |
| **Observable** | Every node lifecycle transition emits an event. The event journal is the audit trail and the basis for live run views. |
| **Governed** | Human gates suspend the run (`run_suspended`) and resume it (`run_resumed`). Sensitive tool calls route through an approval gate. An agent never approves its own output. |
| **Provider-agnostic (BYOM)** | Any model via the environment — native Anthropic & Google Gemini, the OpenAI-compatible family (OpenAI, OpenRouter, MiniMax, Hugging Face, Mistral), and local servers (Ollama, LM Studio). Run on a hosted model or fully on-premise. See [Providers & BYOM](/docs/building/providers). |
| **MCP-native** | Drive Adriane and read its knowledge over the Model Context Protocol: agents and graphs as MCP tools, a knowledge base as MCP resources. See [MCP server](/docs/building/mcp-server). |

## The mental model

A graph in Adriane is three things:

1. **State** — a set of named **channels** with typed values and **reducers** that say how
   a node's output is merged in (replace, append, merge).
2. **Nodes** — units of work: an *action* (a function over state), an *agent* (a ReAct loop
   over an LLM), a *tool node*, or a *human gate*.
3. **Edges** — how control flows between nodes: plain edges, **conditional edges** (a named
   predicate over the state), and fan-out (`send`).

```text
        ┌──────────┐   needsReview    ┌────────────┐   approve    ┌──────────┐
input → │  agent   │ ───────────────▶ │ human gate │ ───────────▶ │ publish  │ → done
        └──────────┘                  └────────────┘              └──────────┘
              │ isClean                       ▲
              └───────────────────────────────┘  (suspends here; resumes from checkpoint)
```

The run executes one node at a time, checkpointing as it goes. When it reaches a human
gate it **suspends** — the process can exit entirely — and a later **resume** picks up from
the checkpoint, emitting the events that bring observers back in sync.

## One engine, two SDKs

Adriane's graph model, validator, and DSL compiler live **once, in Rust**. The TypeScript
and Python SDKs are thin shims over that single engine — not re-implementations. A graph
that validates one way in TypeScript validates exactly the same way in Python, because
there is no second source of truth to drift.

- **TypeScript** — `npm i @adriane-ai/graph-sdk`. The Rust engine (`@adriane-ai/napi`) is a
  **required dependency**, installed with the SDK; Adriane runs on Rust.
- **Python** — `pip install adriane-ai`, then `import adriane_ai`. The wheel ships the
  compiled Rust extension, so the engine is always present.

See [One engine, two languages](/docs/sdk-parity/one-engine-two-languages) for the full
parity story.

## Where to go next

- **[Installation](/docs/getting-started/installation)** — set up TypeScript or Python (or both).
- **[Your first run](/docs/getting-started/your-first-run)** — build and run a graph in a few lines.
- **[Core concepts](/docs/core-concepts/graphs-nodes-edges-state)** — the model in depth.
- **[Governance](/docs/governance/governance-model)** — the differentiator, end to end.
