---
sidebar_position: 2
title: Production best practices
description: Durable checkpoints, governed tools, pinned tiers, and idempotent side effects.
---

# Production best practices

These are the things that bite once a run survives a process restart, a crash, or a
human approval that lands an hour later. None of them are new APIs — they are how to
use the [execution contract](/docs/core-concepts/execution-contract) under real load.

## Use `PgCheckpointer`, not `InMemoryCheckpointer`

A checkpoint is the full typed state plus the position in the graph, written after every
node completion. `InMemoryCheckpointer` keeps those in process memory — fine for tests
and single-process runs, useless the moment the process exits.

For anything that suspends on a human gate, runs on a worker, or must survive a restart,
use `PgCheckpointer`. It persists checkpoints to Postgres, so a run suspended in one
process resumes in **another** process — which is exactly what a worker fleet does.

:::warning In-memory checkpoints do not cross processes
A run suspended on a human gate with `InMemoryCheckpointer` is unresumable from any
other process — including the worker that picks it up next. If `resume()` fails in a new
process, this is almost always why. See
[persistent checkpointing](/docs/core-concepts/resumability-and-approvals).
:::

## Route sensitive tools through approval gates

Any tool with a real-world side effect (a refund, a deploy, an outbound message) should
suspend for human approval rather than fire autonomously. Configure the agent node with
`suspendForApproval` and gate the tool; the run suspends (`run_suspended`), files an
approval request, and resumes only after a human resolves it. The full loop is in
[approval gates](/docs/governance/approval-gates).

## Never let an agent approve its own output

The approver must be a different principal from the requester. The engine enforces this
— a self-approval attempt is rejected (the control plane returns `409`). Do not work
around it by resolving approvals with the same identity that requested them; that
defeats the entire governance story. Review is always a separate principal (see
[agent nodes](/docs/building/agent-nodes-and-react)).

## Pin tiers, not hardcoded models

Declare a capability **tier** (`"frontier" | "balanced" | "fast" | "creative"`) on agent
nodes and let the engine resolve it against the providers available in the environment.
Hardcoding a concrete model id couples your graph to one vendor's catalog and breaks
when that model is retired.

```ts
createGraph({ name: "writer" })
  .agentNode("draft", {
    llm: gateway,
    prompt: { system: "Draft a release note." },
    tier: "balanced"   // resolved per-env, not pinned to one model
  })
  .compile();
```

Expected result: on the Rust path the tier resolves from the process env (e.g. with only
`MISTRAL_API_KEY` set, every tier maps to the Mistral column); on the TS path the SDK
resolves it against `availableFromEnv()`. An explicit `model` always overrides the tier
— use that only when you genuinely need a specific model.

## Keep provider keys in env

`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, and `MISTRAL_API_KEY` are read **only** by the
LLM gateway, and only from the environment. Never hardcode a key in a graph, a prompt,
or source. Agents reference prompts by id/version and models by tier — neither carries a
secret.

## Set a recursion limit

Cyclic graphs are allowed (an agent can loop), so bound them. `createGraph({ recursionLimit })`
caps the loop; exceeding it stops the run with a `RecursionLimitError` rather than
spinning forever. Pick a limit that comfortably covers your longest legitimate
agent loop and no more — an unbounded loop is an unbounded bill.

## Make node side effects idempotent

Because the runtime checkpoints after every node and resumes from the **latest**
checkpoint, completed work is never re-run on a clean resume. But a node can still be
**retried** — a crash mid-node, a worker that dies before the checkpoint is written, a
job re-delivered by BullMQ. If a node's side effect is a charge or an email, design it
to be safe to run twice:

- Use an idempotency key tied to the run + node id so a retried charge is deduplicated
  by the downstream service.
- Prefer "request, then confirm" over "fire and forget": do the irreversible action,
  checkpoint, then act on the confirmation in a later node.
- Gate the truly irreversible actions behind an approval so a human, not a retry,
  triggers them.

:::note The contract protects completed work, not in-flight work
Checkpoint-after-completion guarantees a node that *finished* is not re-run. It cannot
un-send an email a node sent and then crashed before checkpointing. Idempotency is your
responsibility inside the handler.
:::

## Observe via the event journal

Every node lifecycle transition emits an event (`node_started`, `node_completed`,
`run_suspended`, `run_resumed`, `run_completed`, `run_failed`). The persisted event
journal **is** the audit trail and the source of truth for the live run view — if a
transition happened there is an event for it. Read it via the API (`GET /runs/:id/events`
for the durable log, `GET /runs/:id/stream` for the live SSE feed) rather than scraping
application logs. Wire `OTEL_ENDPOINT` if you want the same signals in your collector.
See [observable runs](/docs/governance/observable-runs).

## Next

- [Troubleshooting](/docs/production/troubleshooting)
