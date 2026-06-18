# @adriane-ai/graph-sdk

The front door to the [Adriane](https://github.com/prxmat/adriane-engine) framework: build,
compile and run **stateful, resumable agent graphs** — agents, tools, human-approval
gates, artifacts and long-running workflows — without touching the lower-level engine.

Every run is deterministic by default, checkpointed after every step, and resumable
from where it stopped, including across process restarts and human approvals.

## Install

```bash
npm install @adriane-ai/graph-sdk
# or: pnpm add @adriane-ai/graph-sdk   /   yarn add @adriane-ai/graph-sdk
```

This package is a **self-contained bundle** — it ships the framework inlined and only
pulls a few well-known runtime dependencies (`zod`, `@anthropic-ai/sdk`, `pg`,
`drizzle-orm`). Out of the box it runs on the bundled TypeScript engine.

### Optional: the Rust engine

Graph **execution** can run on Adriane's Rust engine for speed and determinism. Install
the native addon alongside the SDK and it is picked up automatically (with a clean
fallback to the TypeScript engine when it is absent or your platform is unsupported):

```bash
npm install @adriane-ai/napi
```

```ts
import { rustEngineAvailable } from "@adriane-ai/graph-sdk";
console.log(rustEngineAvailable()); // true when the native addon loaded
```

## Quickstart

```ts
import { createGraph } from "@adriane-ai/graph-sdk";

const app = createGraph({ name: "greeter" })
  .node("hello", async (_input, state) => ({
    greeting: `Hello, ${(state.channels as Record<string, unknown>).name}!`
  }))
  .compile();

const result = await app.run({ name: "Ada" });
console.log(result.channels.greeting); // "Hello, Ada!"
```

Add a human-approval gate and the run **suspends** cleanly, then **resumes** from its
checkpoint:

```ts
const app = createGraph({ name: "publish-flow" })
  .node("write", async () => ({ draft: "Hello from Adriane." }))
  .humanGate("review")
  .node("publish", async () => ({ approved: true }))
  .edge("write", "review")
  .edge("review", "publish")
  .compile();

const suspended = await app.run();              // status: "suspended"
const done = await app.resume(suspended.runId); // status: "completed"
```

Conditional routing is always a **named predicate** — never an `eval`'d string — which
is what keeps Adriane's flows safe and inspectable.

## License

Apache-2.0. See [`LICENSE`](./LICENSE).
