# Tutorial 06 — Streaming

**Objective.** Observe a run as it executes. You'll learn three observation surfaces:

1. `stream(initialData, mode)` — an async iterable of `StreamEvent`s as the graph runs.
2. `onEvent(handler)` — subscribe to the run-event **lifecycle** (node started/completed/…).
3. `streamAgentTokens(config, input)` — stream a single agent reply **token by token** for a
   live chat UI.

Prerequisites: [Tutorial 02](./02-agent-nodes.md).

## Streaming a run

`stream(initialData, mode, options?)` returns an `AsyncIterable<StreamEvent>`. The `mode`
selects the shape of events:

| Mode | Emits |
| --- | --- |
| `"values"` | the full `GraphState` (`{ type: "state_value", state }`) |
| `"updates"` | per-node deltas (`{ type: "state_update", delta, nodeId }`) |
| `"messages"` | extracted message deltas (`{ type: "message_delta", delta, nodeId, messageId }`) |
| `"debug"` | debug payloads per node (`{ type: "debug", payload, nodeId }`) |

```ts
import { createGraph } from "@adriane-ai/graph-sdk";

const app = createGraph({ name: "pipeline" })
  .channel("n", { type: "number", default: 0 })
  .node("a", async (_i, s) => ({ n: s.channels.n + 1 }))
  .node("b", async (_i, s) => ({ n: s.channels.n * 10 }))
  .edge("a", "b")
  .compile();

for await (const event of app.stream({ n: 1 }, "updates")) {
  if (event.type === "state_update") {
    console.log(event.nodeId, event.delta);
  }
}
```

**Expected result (TypeScript engine):** a `state_update` per node as it completes —
`a { n: 2 }` then `b { n: 20 }`.

> **Engine note.** The Rust engine has **no incremental stream surface yet**. When running on
> Rust, `stream()` drives a full run and yields a **single terminal** `state_value` event. On
> the TypeScript engine it streams natively per the chosen `mode`. The method signature and
> the `StreamEvent` shapes are identical across engines — only the granularity differs. Check
> `app.usesRustEngine` to know which you're on. To force the TS engine for fine-grained
> streaming during development, set `ADRIANE_SDK_ENGINE=ts`.

`StreamEvent` is a discriminated union, so narrow on `event.type`:

```ts
for await (const event of app.stream({ n: 1 }, "values")) {
  if (event.type === "state_value") {
    console.log(event.state.status, event.state.channels);
  }
}
```

The `StreamEvent` variants are `state_value`, `state_update`, `message_delta`, `tool_call`,
and `debug`.

## Subscribing to lifecycle events

`onEvent(handler)` subscribes to the **run-event lifecycle** and returns an unsubscribe
function. Events flow from **either engine** through the same bus, so this works identically on
Rust and TypeScript:

```ts
const unsubscribe = app.onEvent((event) => {
  console.log(event.type); // e.g. "node_started", "node_completed", "run_completed"
});

await app.run({ n: 1 });
unsubscribe();
```

The `RunEvent` types you'll see: `node_started`, `node_completed`, `node_failed`,
`run_suspended`, `run_resumed`, `run_completed`, `run_failed`. Every node lifecycle transition
emits one — that's the same vocabulary the audit/observability layer records. Subscription is
fire-and-forget (synchronous), so keep handlers cheap.

This is the right surface for governance dashboards and audit trails: a `run_suspended` event
marks where a run paused for approval; `run_resumed` marks where it continued.

## Streaming agent tokens

For a chat UI you want the agent's reply to appear as it's generated. `streamAgentTokens`
streams a **single-turn (no-tools)** reply token by token through the gateway's `stream()`:

```ts
import { streamAgentTokens, type LLMGateway } from "@adriane-ai/graph-sdk";

async function chat(llm: LLMGateway, question: string) {
  for await (const delta of streamAgentTokens(
    { llm, prompt: { system: "You are a helpful assistant." } },
    question
  )) {
    process.stdout.write(delta); // text deltas as they arrive
  }
}
```

`StreamAgentConfig` is `{ llm, prompt, provider?, model? }`. It yields text deltas and returns
when the provider signals done. (This is the single-turn path — use a tool-calling `agentNode`
inside a graph when you need tools; see [Tutorial 03](./03-tools-and-tool-nodes.md).)

## Putting it together

A common pattern: drive the run with `run()` for the result, **and** subscribe with `onEvent`
to feed a live status panel:

```ts
const log: string[] = [];
const off = app.onEvent((e) => log.push(e.type));
const result = await app.run({ n: 1 });
off();
console.log(result.status, log); // "completed", ["node_started", "node_completed", ..., "run_completed"]
```

## Next

[Tutorial 07 — Python SDK](./07-python-sdk.md): the same engine, from Python.
