# Tutorial 05 — Checkpointing and resume

**Objective.** Understand the contract that makes Adriane reliable: a run is **checkpointed
after every node**, **resumable from the latest checkpoint**, and a suspension (human gate or
agent approval) is just a clean, checkpointed pause you continue with `resume()` /
`approveAndResume()`. You'll see how run ids tie it together.

Prerequisites: [Tutorial 04](./04-human-approval-gates.md).

## The runtime contract

These invariants hold for every run, on either engine:

- **Checkpoint after every node completion and state mutation.** Any interruption returns you
  to the latest checkpoint.
- **An event is emitted for every node lifecycle transition** (see
  [Tutorial 06](./06-streaming.md)).
- **Human-gate nodes suspend cleanly** (`status: "suspended"`), and resume restores from the
  latest checkpoint and advances past the gate.
- **Deterministic by default** — same definition, same inputs, same path. Conditions are named
  predicates, never eval'd code.

You don't call the checkpointer yourself for the common case — `CompiledGraph` wires an
in-memory checkpointer for you. You interact with it through `run` / `resume` /
`approveAndResume`.

## Suspend and resume

A suspension is a normal terminal state of a `run()` call. You inspect it, do whatever
out-of-band work is needed (a human approves), then continue:

```ts
import { createGraph } from "@adriane-ai/graph-sdk";

const app = createGraph({ name: "two-step" })
  .channel("step", { type: "string", default: "start" })
  .node("first", async () => ({ step: "after-first" }))
  .humanGate("gate")
  .node("second", async () => ({ step: "done" }))
  .edge("first", "gate")
  .edge("gate", "second")
  .compile();

const paused = await app.run({}, { runId: "run-001" }); // pass a stable run id
console.log(paused.status);        // "suspended"
console.log(paused.runId);         // "run-001"
console.log(paused.currentNodeId); // "gate"
console.log(paused.channels.step); // "after-first"  ← state up to the gate is persisted

// Later — possibly in a different request handler — resume the same run id.
const finished = await app.resume(paused.runId);
console.log(finished.status);        // "completed"
console.log(finished.channels.step); // "done"
```

**Expected result:** state accumulated **up to** the gate (`step: "after-first"`) is preserved
across the pause; `resume` picks up from there and runs `second` to completion.

## Run ids

- `run()` generates a run id automatically; you can override it with `{ runId }` to correlate
  with an external system (a request id, a job id).
- `resume(runId)` and `approveAndResume(runId, …)` take that id to continue the right run.
- Run ids are stable for checkpointing — reuse the **same** id to resume the **same** run.

> **Rust path requirement.** When running on the Rust engine, `resume` / `approveAndResume`
> must be called on the **same `CompiledGraph` instance** that produced the suspension — that
> instance holds the suspended state and feeds it back to the engine. Calling resume for an
> unknown run id throws a clear error. (The control plane uses a durable checkpointer to span
> processes; that path is internal and not part of the public SDK.)

## Resuming after approval

`approveAndResume` is the resume variant for an agent that suspended for tool approval — it
grants the named tools, then resumes:

```ts
const suspended = await app.run();                        // suspends at the agent node
const done = await app.approveAndResume(suspended.runId, { approvedTools: ["refund"] });
```

See [Tutorial 04](./04-human-approval-gates.md) for the full agent-approval walkthrough.

## Determinism in practice

Because conditions are named predicates and state flows only through declared channels, the
same definition + same inputs produce the same path. This is what lets you replay a run,
resume it after a crash, or branch a decision with a different approval outcome — the engine's
guarantees don't depend on hidden state.

## What persists, and where

- The default `CompiledGraph` uses an **in-memory** checkpointer and event bus — perfect for
  development, tests, and single-process runs.
- For **durable** checkpoints that survive process restarts, bring your own `Checkpointer`
  (the interface is exported) or use the private Postgres-backed adapters in the control plane.
  The public SDK bundle deliberately does **not** embed the database schema.

## Try it

The checkpoint/resume contract is exercised by every suspend/resume example:

```bash
pnpm --filter @adriane-ai/graph-sdk example         # quickstart: suspend then resume
pnpm --filter @adriane-ai/graph-sdk example:agent   # suspend for approval, then approveAndResume
```

## Next

[Tutorial 06 — Streaming](./06-streaming.md): observe a run as it executes.
