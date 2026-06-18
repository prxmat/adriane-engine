---
sidebar_position: 4
title: Resumability and approvals
description: How a run suspends at a human gate and resumes exactly from the latest checkpoint.
---

# Resumability and approvals

Because Adriane checkpoints after every node, a run can **stop and continue later** without
losing or repeating work. The headline use of that capability is the **human gate**: a node
that suspends the run for a human decision.

## Suspend and resume

A `humanGate` node suspends the run cleanly. The run's `status` becomes `"suspended"` and
`currentNodeId` points at the gate. The process can exit entirely — the state is checkpointed.

```ts
import { createGraph } from "@adriane-ai/graph-sdk";

const app = createGraph({ name: "publish-flow" })
  .node("write", async () => ({ draft: "…" }))
  .humanGate("review")
  .node("publish", async () => ({ published: true }))
  .edge("write", "review")
  .edge("review", "publish")
  .compile();

const first = await app.run({});
console.log(first.status);        // "suspended"
console.log(first.currentNodeId); // "review"
```

Later — same process or a fresh one, as long as the checkpoint is durable — you resume:

```ts
const resumed = await app.resume(first.runId);
console.log(resumed.status);      // "completed"
```

The runtime emits `run_suspended` when it parks at the gate and `run_resumed` when it picks
back up, then continues `publish → run_completed`. Observers replaying the event journal see
exactly that sequence.

## Durable checkpoints

In-memory checkpointing is fine for a single process. To suspend in one process and resume in
another — the realistic shape of a human-approval workflow — use a durable checkpointer:

```ts
import { PgCheckpointer } from "@adriane-ai/graph-sdk";

const app = createGraph({ name: "publish-flow" })
  .checkpointer(new PgCheckpointer({ connectionString: process.env.DATABASE_URL }))
  // …nodes…
  .compile();
```

Now `app.resume(runId)` works across process boundaries: the API suspends a run, a human
approves hours later, and a worker resumes it from the persisted checkpoint.

## Approvals are governed resumes

A human gate is the mechanism; **governance** is the policy layered on top. A gate can require
an explicit *approval*: a decision recorded by an authenticated principal who is **not** the
agent that requested it, attested and audited. That full loop — request, approve/reject,
attest, resume — is the subject of the [Governance](/docs/governance/governance-model) section.

## Next

- [Runtime and engine](./runtime-and-engine)
- [Approval gates](/docs/governance/approval-gates)
