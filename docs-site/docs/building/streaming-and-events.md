---
sidebar_position: 4
title: Streaming and events
description: Observe a run as it executes — stream modes, lifecycle events, and agent tokens.
---

# Streaming and events

Three surfaces let you observe a run:

1. `stream(initialData, mode)` — an async iterable of `StreamEvent`s as the graph runs.
2. `onEvent(handler)` — subscribe to the run-event **lifecycle**.
3. `streamAgentTokens(config, input)` — stream a single agent reply **token by token**.

## Streaming a run

`stream(initialData, mode, options?)` returns an `AsyncIterable<StreamEvent>`. The `mode`
selects the event shape:

| Mode | Emits |
| --- | --- |
| `"values"` | the full `GraphState` (`{ type: "state_value", state }`) |
| `"updates"` | per-node deltas (`{ type: "state_update", delta, nodeId }`) |
| `"messages"` | extracted message deltas (`{ type: "message_delta", … }`) |
| `"debug"` | debug payloads per node (`{ type: "debug", payload, nodeId }`) |

```ts
for await (const event of app.stream({ n: 1 }, "updates")) {
  if (event.type === "state_update") console.log(event.nodeId, event.delta);
}
```

`StreamEvent` is a discriminated union (`state_value`, `state_update`, `message_delta`,
`tool_call`, `debug`) — narrow on `event.type`.

:::note Engine note
The Rust engine has **no incremental stream surface yet**: on Rust, `stream()` drives a full
run and yields a **single terminal** `state_value`. On the TypeScript engine it streams natively
per `mode`. The signature and `StreamEvent` shapes are identical across engines — only the
granularity differs. Check `app.usesRustEngine`; set `ADRIANE_SDK_ENGINE=ts` to force the TS
engine for fine-grained streaming in development.
:::

## Subscribing to lifecycle events

`onEvent(handler)` subscribes to the run-event lifecycle and returns an unsubscribe function.
Events flow from **either engine** through the same bus:

```ts
const unsubscribe = app.onEvent((event) => {
  console.log(event.type); // "node_started", "node_completed", "run_completed", …
});

await app.run({ n: 1 });
unsubscribe();
```

The `RunEvent` vocabulary: `node_started`, `node_completed`, `node_failed`, `run_suspended`,
`run_resumed`, `run_completed`, `run_failed`. Every node lifecycle transition emits one — the
same vocabulary the [audit/observability layer](/docs/governance/observable-runs) records.
Subscription is synchronous (fire-and-forget), so keep handlers cheap.

This is the right surface for governance dashboards: `run_suspended` marks where a run paused
for approval, `run_resumed` where it continued.

## Streaming agent tokens

For a chat UI, stream the agent's reply as it's generated. `streamAgentTokens` streams a
**single-turn (no-tools)** reply token by token:

```ts
import { streamAgentTokens, type LLMGateway } from "@adriane-ai/graph-sdk";

for await (const delta of streamAgentTokens(
  { llm, prompt: { system: "You are a helpful assistant." } },
  "What is Adriane?"
)) {
  process.stdout.write(delta);
}
```

`StreamAgentConfig` is `{ llm, prompt, provider?, model? }`. For tools, use a tool-calling
`agentNode` inside a graph (see [tools](./tools-and-tool-nodes)).

## Putting it together

Drive the run with `run()` for the result **and** subscribe with `onEvent` for a live panel:

```ts
const log: string[] = [];
const off = app.onEvent((e) => log.push(e.type));
const result = await app.run({ n: 1 });
off();
console.log(result.status, log); // "completed", ["node_started", …, "run_completed"]
```

## Next

[The Adriane DSL →](/docs/dsl/graph-yaml-syntax)
