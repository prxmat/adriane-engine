---
sidebar_position: 3
title: Tool approval and attestation
description: The reserved channels that carry approvals, and how decisions are attested.
---

# Tool approval and attestation

This page is the wire-level companion to [approval gates](./approval-gates): how an approval
actually reaches the engine, and how the decision is made tamper-evident.

## Reserved channels

Two channel names are **reserved** by the engine to carry approval state across a
suspend/resume boundary. They are signalled early so the engine can route them without
guessing:

| Reserved channel | Carries |
| --- | --- |
| `__approvedTools` (`APPROVED_TOOLS_CHANNEL`) | The tool **names** a human granted for this run. |
| `__approvalIds` (`APPROVAL_IDS_CHANNEL`) | The ids of the approval records that authorised them. |

You don't write these by hand. `approveAndResume(runId, { approvedTools })` populates them:

- On the **Rust** path, the engine writes the approved tool names into `__approvedTools` before
  resuming.
- On the **TypeScript** path, the channel is updated directly.

Either way, when the agent re-runs it sees the approved tools in the reserved channel and
executes exactly those — no re-gating, no self-grant.

## The approval record

Each gated request produces an **approval record** with a stable id. Its shape (as surfaced by
the control plane):

```json
{
  "id": "<approvalId>",
  "requestedBy": "assistant",     // the agent — the requester
  "subject": "tool:refund",       // what is being approved
  "status": "pending",            // → "approved" | "rejected"
  "resolvedBy": null              // set to the authenticated approver on resolution
}
```

`resolvedBy` is bound to the **authenticated principal** that resolves the request — never a
free-text field, and never the requester (that is the
[no-self-approval](./approval-gates#no-self-approval) rule).

## Attestation

When a decision is recorded, it is **attested with an Ed25519 signature**. The signature binds
the decision (the approval id, the subject, the resolver, the outcome) to the holder of a
specific signing key. After the fact you can verify that:

- the decision was made by the holder of that key, and
- the recorded decision has not been altered.

Combined with the [event journal](./observable-runs) — which captures the suspend, the pending
request, the resolution, and the resume in order — you get an audit trail that is both
**complete** (every transition is an event) and **tamper-evident** (every decision is signed).

## End to end

```text
agent → gated tool `refund`
   └─ run_suspended ─────────────▶ approval record { requestedBy: assistant, subject: tool:refund, pending }
                                      │
        human (≠ agent) approves  ────┤  resolvedBy = <principal>;  decision attested (Ed25519)
                                      ▼
        approveAndResume  ──▶ __approvedTools = ["refund"]  ──▶ agent re-runs, `refund` executes  ──▶ run_completed
```

## Next

- [Observable runs](./observable-runs) — surface all of this as a live view.
- [Approval gates](./approval-gates) — the builder-side API.
