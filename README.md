# Adriane

**The open framework for stateful, resumable agent graphs — with a governed studio on top.**

Adriane orchestrates agents, tools, human-approval gates, artifacts and
long-running workflows as a **graph**. The execution engine is written in **Rust**
(see [`crates/`](crates/)) and driven through thin **TypeScript and Python SDKs**.
Every run is deterministic by default, checkpointed after every step, and resumable
from where it stopped — including across process restarts and human approvals.

Think of it the way [Haystack](https://haystack.deepset.ai/) splits its world:

| | Adriane |
| --- | --- |
| **The framework** (open source, self-hostable) | The Rust engine, the SDKs (TypeScript + Python) and the CLI. Build and run graphs anywhere — your laptop, your servers, your CI. |
| **Adriane Studio** (commercial, hosted) | The visual control plane: graph builder, fleet management, observability/tracing, evaluation, multi-tenant governance and approvals. |

The framework is, and stays, open. Studio is the paid product built on top of it.

---

## Quickstart

> **Pre-release.** Not published to npm/PyPI yet — install from source.

```bash
git clone https://github.com/prxmat/adriane-engine.git && cd adriane-engine
pnpm install
# run a tutorial right away (mock LLM, no API key, self-verifying):
pnpm --filter @adriane/graph-sdk example
```

Then define a graph (the SDK resolves from the workspace):

```ts
import { createGraph } from "@adriane/graph-sdk";

const app = createGraph({ name: "greeter" })
  .node("hello", async (_input, state) => ({
    greeting: `Hello, ${(state.channels as Record<string, unknown>).name}!`
  }))
  .compile();

const result = await app.run({ name: "Ada" });
console.log(result.channels.greeting); // "Hello, Ada!"
```

Add a human-approval gate and the run **suspends** cleanly, then **resumes** from
its checkpoint:

```ts
const app = createGraph({ name: "publish-flow" })
  .node("write", async () => ({ draft: "Hello from Adriane." }))
  .humanGate("review")
  .node("publish", async () => ({ approved: true }))
  .edge("write", "review")
  .edge("review", "publish")
  .compile();

const suspended = await app.run();   // status: "suspended"
const done = await app.resume(suspended.runId); // status: "completed"
```

Conditional routing is always a **named predicate** — never an `eval`'d string —
which is what keeps Adriane's flows safe and inspectable:

```ts
createGraph({ name: "router" })
  .node("triage", async () => ({ score: 0.9 }))
  .node("escalate", async () => ({}))
  .node("autoresolve", async () => ({}))
  .conditionalEdge("triage", "escalate", "isRisky", (s) => Number(s.channels.score) >= 0.8)
  .conditionalEdge("triage", "autoresolve", "isSafe", (s) => Number(s.channels.score) < 0.8)
  .compile();
```

Five runnable tutorials ship with the SDK — no API key required (they use a mock LLM),
and each one is self-verifying, so they double as end-to-end tests:

```bash
pnpm --filter @adriane/graph-sdk example           # Beginner — suspend/resume with a human gate
pnpm --filter @adriane/graph-sdk example:agent     # Intermediate — a ReAct agent routed into an approval gate
pnpm --filter @adriane/graph-sdk example:qa        # Intermediate — QA over documents, citations + low-confidence gate
pnpm --filter @adriane/graph-sdk example:startup   # Advanced — idea → ship: a governed venture pipeline
pnpm --filter @adriane/graph-sdk example:finance   # Advanced — optimisation des flux finance (export Sage)
```

See the Haystack-style tutorials index in
[`packages/graph-sdk/examples/README.md`](packages/graph-sdk/examples/README.md). Channel value
types flow through the builder, so handler state and the result of `run`/`resume`
are fully typed with no manual annotation.

## What you get from the framework

- **Deterministic, resumable runs** — checkpoint after every node and state
  mutation; resume from the latest checkpoint, including after suspension.
- **Human-in-the-loop gates** — `human-gate` nodes suspend the run and wait for
  approval before continuing.
- **Streaming** — observe `values`, `updates`, `messages` or `debug` events as a
  graph executes.
- **Time-travel, fan-out/`send`, cycles, subgraphs and tool nodes** — the full
  runtime contract, framework-agnostic and Vitest-covered.
- **Safe by construction** — no `eval` / `new Function` / dynamic `import()` of
  user strings; conditions are named registry predicates; agents cannot approve
  their own outputs; sensitive actions route through approval gates.

## Architecture

This repository **is** the framework. The Rust engine lives in `crates/`, the
TypeScript and Python SDKs and supporting packages in `packages/`.

```
crates/     the Rust execution engine  (OPEN SOURCE)
packages/   the framework              (OPEN SOURCE)
  graph-core       pure data model: GraphDefinition, channels, validation, errors
  graph-runtime    the execution engine: checkpoints, events, suspend/resume, streaming
  graph-sdk        ← you are here: the ergonomic front door
  agents-core      agent patterns (ReAct, plan-execute, reflection, supervisor, swarm)
  llm-gateway      the only package allowed to import provider SDKs
  lang-/graph-adriane   the Adriane DSL compilers (YAML → graph)
  artifact-store · approval-engine · observability · memory-store · rag-pipeline
  adriane-cli      run and inspect graphs from the terminal
```

> Everything in this repository is the open framework. The SDK is the supported,
> stable surface — import `@adriane/graph-sdk`, not a package's internals.
> **Adriane Studio**, the hosted commercial control plane (visual builder, fleet,
> tracing, evaluation, multi-tenant governance), is a separate product built on top
> of this framework and is not part of this repository.

## Engine: Rust (TS engine deprecated, fallback only)

Graph **execution** now runs on the Rust engine in [`crates/`](crates/), reached from
`@adriane/graph-sdk` through the `@adriane/napi` native addon (an async bridge that
calls back into JS condition/node/tool seams over a ThreadsafeFunction). The SDK runs
on Rust when the native addon is present and **falls back to the TypeScript engine**
when it is absent — so nothing breaks if the addon is unbuilt.

As a result the TypeScript engine packages — `graph-runtime`, `agents-core`,
`llm-gateway`, `approval-engine`, `memory-store`, `artifact-store`, `callbacks`,
`observability`, `runnable`, `rag-pipeline`, `lang-adriane`, `graph-adriane` — are
**deprecated as execution engines** and retained only as that fallback. `graph-sdk`
(the front door) and `graph-core` (the shared data model + validator) are **not**
deprecated. Import `@adriane/graph-sdk`; do not depend on the engine packages directly.
See [`docs/adr/0003-ts-engine-deprecated-sdk-on-rust.md`](docs/adr/0003-ts-engine-deprecated-sdk-on-rust.md).

## Development

```bash
pnpm install
pnpm build        # turbo run build (respects ^build order)
pnpm test         # vitest across all workspaces
pnpm typecheck
pnpm lint
pnpm rust:check   # cargo fmt + clippy -D warnings + tests, using Cargo.lock

# scope to one package
pnpm --filter @adriane/graph-sdk test
```

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the working conventions and
`.cursor/rules/*.mdc` for the authoritative per-layer rules.

## License

The framework (`crates/*`, `packages/*`, `plugin/*`) is open source under
Apache-2.0. See [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE) for details. Adriane
Studio is a separate commercial product and is not part of this repository.
