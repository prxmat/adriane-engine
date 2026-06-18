---
sidebar_position: 5
title: Runtime and engine
description: The Rust engine — the required runtime — and the internal TypeScript engine for development and tests.
---

# Runtime and engine

Adriane has **one engine, written in Rust**, and **two SDK surfaces** over it. Understanding
where execution actually happens explains the framework's performance story and its
cross-language guarantees.

## The Rust engine

The graph model, the validator, the DSL compiler, the model policy, and the component and
prebuilt catalogs all live once, in Rust (`crates/`). This is the single source of truth.
There is no parallel TypeScript or Python re-implementation to drift out of sync.

## Two surfaces over it

- **Python** reaches the engine through a [pyo3](https://pyo3.rs) extension
  (`crates/py-bindings`). The wheel ships the compiled engine, so it is **always present** —
  there is no fallback path. `import adriane_ai` *is* the engine.
- **TypeScript** reaches the engine through a [napi-rs](https://napi.rs) addon
  (`crates/bindings`, published as `@adriane-ai/napi`). This addon is a **required
  dependency** of `@adriane-ai/graph-sdk` — installed automatically, never opt-in.

## Rust is the runtime

Adriane runs on the Rust engine. Because `@adriane-ai/napi` is a regular dependency of the SDK,
a single `npm i @adriane-ai/graph-sdk` installs it and the native engine is active:

```ts
import { rustEngineAvailable } from "@adriane-ai/graph-sdk";

console.log(rustEngineAvailable()); // true — the Rust engine is running
```

## The internal TypeScript engine (development & tests)

An in-process TypeScript engine backs **development, the test suite, and the platforms the
native addon doesn't cover yet**. It produces **identical observable behaviour** — the same
final status, the same suspend/resume points, the same lifecycle events — so tests exercise the
same contract. It is not a runtime you target: production is Rust. Your code, and any observer
of a run, cannot tell which implementation executed.

## The bridge

The native boundary is narrow and JSON-shaped. The addon exposes a handful of entry points —
graph validation, DSL compilation, the model policy, and the fully-Rust run paths — and three
callbacks let the engine call back into the host for node handlers, condition predicates, and
event emission. The bridge is detailed in
[Architecture → the native bridge](/docs/architecture/napi-bridge).

## Practical implications

- **In TypeScript**, the Rust addon is a required dependency — it's there after a normal
  install. Don't branch your application logic on `rustEngineAvailable()` for *correctness* —
  only, at most, for diagnostics.
- **In Python**, the engine is always there too; the wheel ships it, so there is nothing to
  install separately.
- **Across both**, a graph that validates or compiles one way validates and compiles the same
  way in the other, because the same Rust code answers in both.

## Next

- [SDK parity → one engine, two languages](/docs/sdk-parity/one-engine-two-languages)
- [Architecture overview](/docs/architecture/overview)
