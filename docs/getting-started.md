# Getting started

Goal: clone the repo, build, and get your **first graph run green in under 10 minutes** —
fully offline, no API keys.

> **Pre-release.** Adriane is not on npm/PyPI yet. You install **from source** in this
> monorepo and run the SDK from the workspace.

## Prerequisites

| Tool | Version |
| --- | --- |
| Node | `>= 22` |
| pnpm | `10.11.0` |
| Rust | `1.96.0` (pinned in `rust-toolchain.toml`) — only needed to build the native engine |
| Docker | Only needed for the control plane (API/Studio/Worker), **not** for the engine SDK |

You can complete this whole guide with **just Node + pnpm**. Rust is optional — without the
native addon the SDK runs on its bundled TypeScript engine (you'll see one warning per run).

## 1. Clone and install

```bash
git clone https://github.com/prxmat/adriane-engine.git
cd adriane-engine
pnpm install
```

## 2. Build the workspace

```bash
pnpm build        # turbo run build across all workspaces (respects ^build order)
```

## 3. (Optional) build the native Rust engine

To run on the Rust engine instead of the TypeScript fallback, build the native addon:

```bash
pnpm napi:build   # compiles crates/bindings → adriane_napi.node
```

Verify it loaded:

```ts
import { rustEngineAvailable } from "@adriane-ai/graph-sdk";
console.log(rustEngineAvailable()); // true when the native addon is present
```

If you skip this step, everything below still works — it just runs on the TypeScript engine.

## 4. Run a shipped example (the fastest first run)

The SDK ships runnable, self-verifying examples. They assert their own behaviour and exit
non-zero on the first failed assertion, so a clean exit means everything worked.

```bash
pnpm --filter @adriane-ai/graph-sdk example          # quickstart: human gate + resume
pnpm --filter @adriane-ai/graph-sdk example:agent    # agent + approval gate
pnpm --filter @adriane-ai/graph-sdk example:qa        # governed QA over documents
```

Expected output for `example` (the quickstart):

```
suspended
completed
true
```

That's a full lifecycle: the run **suspended** at a human-approval gate, you **resumed** it
from its checkpoint, and it **completed**.

## 5. Write your own first run

Create a file `hello.ts` anywhere inside the workspace:

```ts
import { createGraph } from "@adriane-ai/graph-sdk";

const app = createGraph({ name: "greeter" })
  .channel("name", { type: "string", default: "" })
  .channel("greeting", { type: "string", default: "" })
  .node("hello", async (_input, state) => ({ greeting: `Hello, ${state.channels.name}!` }))
  .compile();

const result = await app.run({ name: "Ada" });
console.log(result.status);            // "completed"
console.log(result.channels.greeting); // "Hello, Ada!"
```

Run it from inside the SDK package so the workspace resolution picks up `@adriane-ai/graph-sdk`:

```bash
pnpm --filter @adriane-ai/graph-sdk exec node --import tsx /absolute/path/to/hello.ts
```

Expected output:

```
completed
Hello, Ada!
```

## What just happened

- `createGraph({ name })` started a **fluent, typed builder**.
- `.channel(...)` declared two typed state channels. The `greeting` channel's `string` type
  flows through to `result.channels.greeting` — no manual annotation needed.
- `.node("hello", handler)` registered an **action node**; the first node added is the entry
  point by default.
- `.compile()` validated the graph (on the Rust core when present, else the TS validator) and
  returned a runnable `CompiledGraph`.
- `app.run({ name: "Ada" })` started a fresh run, checkpointed after the node, and returned
  the typed terminal state.

## Next

Continue to **[Tutorial 01 — Your first graph](./tutorials/01-your-first-graph.md)** to add
edges, conditional routing, and understand reducers.
