---
sidebar_position: 1
title: One engine, two languages
description: The same Rust core under TypeScript and Python — what's shared, what differs, and why.
---

# One engine, two languages

Adriane is **one Rust engine** with **two SDK surfaces**. This page is the contract between
them: what is guaranteed identical, where the surfaces deliberately differ, and why.

## What's shared — the engine

The graph model, the validator, the DSL compiler, the model policy, and the component and
prebuilt catalogs live once, in Rust. Both SDKs call into that same code:

- A graph that **validates** in TypeScript validates identically in Python.
- A DSL document that **compiles** one way compiles the same way in the other.
- `resolve_model` / model-policy decisions are the same given the same environment.
- The component and prebuilt catalogs are the same lists.

There is no second implementation to drift. This is the whole point of the design.

## What differs — the surface

| | TypeScript (`@adriane-ai/graph-sdk`) | Python (`adriane-ai`) |
| --- | --- | --- |
| Install / import | `npm i @adriane-ai/graph-sdk` · `import { createGraph } from "@adriane-ai/graph-sdk"` | `pip install adriane-ai` · `import adriane_ai` |
| Rust engine | required dependency (`@adriane-ai/napi`), installed with the SDK | built into the wheel, always present |
| Builder + custom node handlers | ✅ full builder, custom JS handlers, conditional predicates | ❌ no custom nodes (see below) |
| Streaming | ✅ | ❌ |
| Validate / compile DSL | ✅ | ✅ |
| Model policy | ✅ | ✅ |
| Component & prebuilt catalogs | ✅ | ✅ run via `run_component` / `run_prebuilt` |
| Fully-Rust run paths | ✅ | ✅ |

### Why Python has no custom nodes

The Python binding is **JSON-in / JSON-out** and runs on a single-threaded tokio runtime inside
Rust — **no Python callbacks cross the boundary**. A custom node handler would mean calling
*back* into Python from Rust on every step, which the binding intentionally doesn't do. So the
Python surface is the part of the engine that needs no host callbacks: validation, compilation,
the model policy, single-component runs, and prebuilt-agent runs (which execute end to end in
Rust).

The TypeScript SDK *does* bridge callbacks (node handlers, condition predicates, event
emission), which is why it offers the full builder with custom JavaScript nodes.

## The asymmetry is intentional

This is not "Python is the lesser SDK." It's that each surface exposes what its bridge can do
faithfully:

- Reach for **Python** to validate, compile, and run graphs and prebuilt agents from a Python
  app or notebook — with the real engine, no re-implementation.
- Reach for **TypeScript** when you need to author graphs with custom logic, stream, or embed
  the engine in a Node service.

Either way you are driving the same Rust core.

## Next

- [TypeScript SDK](./typescript-sdk)
- [Python SDK](./python-sdk)
