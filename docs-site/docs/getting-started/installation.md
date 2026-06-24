---
sidebar_position: 1
title: Installation
description: Install the Adriane SDK in TypeScript or Python — names, packages, and the Rust engine.
---

# Installation

Adriane ships **two SDKs over one Rust engine**. Pick your language — the package you
install and the name you import are spelled out below, because they differ by ecosystem
convention.

:::tip Naming at a glance
| | Install | Import | Command |
| --- | --- | --- | --- |
| **TypeScript** | `npm i @adriane-ai/graph-sdk` | `import { createGraph } from "@adriane-ai/graph-sdk"` | — |
| **Python** | `pip install adriane-ai` | `import adriane_ai` | — |
| **CLI** | `npm i -g @adriane-ai/cli` | — | `adriane` |

The npm scope is `@adriane-ai`. On PyPI the distribution is `adriane-ai` (hyphen), but the
**import package is `adriane_ai`** (underscore) — Python module names can't contain a
hyphen, so this is the standard pip↔import split, the same as `pip install scikit-learn` /
`import sklearn`.
:::

## TypeScript

```bash
npm i @adriane-ai/graph-sdk
# or: pnpm add @adriane-ai/graph-sdk   /   yarn add @adriane-ai/graph-sdk
```

```ts
import { createGraph } from "@adriane-ai/graph-sdk";

const app = createGraph({ name: "hello" })
  .node("greet", async () => ({ greeting: "hello world" }))
  .compile();

const result = await app.run({});
console.log(result.status); // "completed"
```

`@adriane-ai/graph-sdk` is a **self-contained bundle**, and it depends on the Rust engine
(`@adriane-ai/napi`) — so `npm i @adriane-ai/graph-sdk` pulls the engine for you. No extra step.

:::tip Fastest start — scaffold a governed app
```bash
npm create adriane@latest my-app   # a runnable governed graph + the dev inspector
cd my-app && npm install && npm start
```
:::

### The Rust engine is required

Adriane runs on the **Rust engine**. `@adriane-ai/napi` is a regular **dependency** of the SDK,
installed automatically; you don't install it separately and you don't opt in to it. You can
confirm it's active:

```ts
import { rustEngineAvailable } from "@adriane-ai/graph-sdk";

console.log(rustEngineAvailable()); // true — the Rust engine is running
```

:::note No TypeScript execution fallback
Execution is **Rust-only**: if the native addon genuinely can't run a graph, the SDK throws
`RustEngineRequiredError` rather than silently degrading (ADR 0016). Prebuilt addons cover
macOS, Linux **glibc**, and Windows (x64/arm64); on an uncovered target (e.g. **musl/Alpine**)
either use a glibc base image (`node:20-slim`) or install the Rust toolchain so the addon builds
from source. The TypeScript engine packages remain only as an internal dev/test aid, never a
runtime you target.
:::

## Python

```bash
pip install adriane-ai
```

```python
import adriane_ai

print(adriane_ai.engine_version())   # the bound Rust engine version
```

A single `cp39-abi3` wheel covers CPython 3.9+ — the extension targets the stable ABI, so
**nothing compiles on your machine** and the Rust engine is **always present** (there is no
fallback path in Python; the wheel *is* the engine).

The Python SDK is a thin JSON-in / JSON-out surface over the engine: graph validation, DSL
compilation, the model policy, the component and prebuilt catalogs, and the fully-Rust run
paths. See the [Python SDK](/docs/sdk-parity/python-sdk) page for the full surface.

## CLI

```bash
npm i -g @adriane-ai/cli
adriane --help
```

The npm package is `@adriane-ai/cli`; the installed command is `adriane`. It bundles the
engine, so it runs the moment it's installed. See [the CLI reference](/docs/cli/commands).

## From source (contributors)

The engine and both SDKs build from the monorepo. You need Node 18+ with pnpm, and a Rust
toolchain for the native engine.

```bash
pnpm install
pnpm build

# Native engine (the SDK's required runtime; also builds the Python wheel locally):
pnpm napi:build   # builds the Node addon  → engine/crates/bindings/adriane_napi.node
pnpm py:build     # builds the Python ext   → python/adriane_ai/adriane.abi3.so
```

## Next

[Your first run →](/docs/getting-started/your-first-run)
