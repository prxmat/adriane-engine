---
sidebar_position: 1
title: Builder API
description: Every GraphBuilder and CompiledGraph method, with signatures and option tables.
---

# Builder API

The fluent surface of `@adriane-ai/graph-sdk`. You build a graph with `createGraph(...)`,
chain `GraphBuilder` methods to declare channels, nodes and edges, then `compile()` into a
runnable `CompiledGraph`.

```ts
import { createGraph } from "@adriane-ai/graph-sdk";

const app = createGraph({ name: "greeter" })
  .node("hello", async (_input, state) => ({ greeting: `Hello, ${state.channels.name}!` }))
  .compile();

const result = await app.run({ name: "Ada" });
console.log(result.channels.greeting);
```

Expected result: prints `Hello, Ada!`.

The `TState` type parameter accumulates the declared channels as you call `.channel(...)`,
so handler state and the result of `run` / `resume` are fully typed without any manual
annotation. The whole API is in `packages/graph-sdk/src/builder.ts` and
`packages/graph-sdk/src/compiled-graph.ts`.

## `createGraph(options)`

```ts
const createGraph = (options: CreateGraphOptions): GraphBuilder<EmptyChannels>;
```

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `name` | `string` | — (required) | Human-readable graph name. |
| `id` | `string` | slugified `name` | Stable graph id. |
| `version` | `string` | `"0.0.0"` | Semver-ish version string. |
| `recursionLimit` | `number` | engine default | Bounds cyclic execution (see [execution contract](/docs/core-concepts/execution-contract)). |
| `metadata` | `Record<string, unknown>` | `undefined` | Arbitrary graph metadata. |

## GraphBuilder methods

Every method except `compile` / `safeCompile` returns the builder for chaining. The
channel-declaring and node-adding methods widen `TState`; `edge` / `conditionalEdge` /
`entry` return `this`.

### `channel(name, definition)`

```ts
channel<TName extends string, TValue = unknown>(
  name: TName,
  definition: ChannelInput<TValue>
): GraphBuilder<TState & { [K in TName]: TValue }>;
```

Declare one state channel. The value type is inferred from `definition.default`.

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `type` | `string` | — (required) | Channel type tag (e.g. `"string"`, `"json"`). |
| `reducer` | `ChannelReducer` | `"replace"` | How concurrent writes merge. See [channels and reducers](/docs/core-concepts/channels-and-reducers). |
| `default` | `TValue` | `undefined` | Initial value; also fixes the inferred type. |

### `messagesChannel(name?)`

```ts
messagesChannel<TName extends string = "messages">(
  name?: TName
): GraphBuilder<TState & { [K in TName]: Message[] }>;
```

Declare an append-reduced `Message[]` channel (the conversational default). Equivalent to
`channel(name, { type: "messages", reducer: "append", default: [] })`. `name` defaults to
`"messages"`.

### `node(id, handlerOrConfig)`

```ts
node(id: string, handlerOrConfig: TypedNodeHandler<TState> | NodeInput<TState>): this;
```

Add a node. Pass a bare handler for the common action case, or a config object. The first
node added becomes the entry node unless you call `entry(...)`.

| `NodeInput` field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `type` | `NodeType` | `"action"` | Node type. |
| `handler` | `TypedNodeHandler<TState>` | — | Required for an `action` node — omitting it throws `MissingHandlerError`. |
| `label` | `string` | `id` | Display label. |
| `retryPolicy` | `RetryPolicy` | `undefined` | Per-node retry behaviour. |
| `metadata` | `Record<string, unknown>` | `undefined` | Arbitrary node metadata. |

Adding two nodes under the same id throws `DuplicateNodeError`.

A handler returns a channel-update object. It may also return a routing `Command`
(`{ goto, update? }`) — but see the [Rust caveat](#rust-engine-caveats) below: a `goto` is
dropped on the Rust path.

### `agentNode(id, config)`

```ts
agentNode<TOut extends string = "agentResult">(
  id: string,
  config: AgentNodeConfig & { outputChannel?: TOut }
): GraphBuilder<TState & { [K in TOut]: AgentResult }>;
```

Add a ReAct agent node. Its `AgentResult` lands in `config.outputChannel` (default
`"agentResult"`), which is auto-declared and added to the typed state. Full walkthrough in
[agent nodes & ReAct](/docs/building/agent-nodes-and-react).

| `AgentNodeConfig` field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `llm` | `LLMGateway` | — (required) | The gateway the agent runs on (TS path only — see caveats). |
| `prompt` | `AgentPromptSource` | — (required) | `{ system }` inline, or `{ registry, id, version? }`. |
| `tools` | `ToolRegistry` | `undefined` | Tools the agent may call. |
| `provider` | `LLMProvider` | `"anthropic"` | Pin the provider. |
| `model` | `string` | `undefined` | Pin the model (always wins over `tier`). |
| `tier` | `ModelTier` | `undefined` | Capability tier: `"frontier" \| "balanced" \| "fast" \| "creative"`. |
| `maxIterations` | `number` | agent default | Cap on the ReAct loop. |
| `name` | `string` | `id` | Agent name; also the approval requester principal. |
| `description` | `string` | `agent node <id>` | Agent description. |
| `outputChannel` | `string` | `"agentResult"` | Channel the result lands in. |
| `suspendForApproval` | `boolean` | `false` | Suspend the run when a gated tool is reached. |
| `approvalEngine` | `ApprovalEngine` | `undefined` | Route approvals through an engine (TS-engine only — see caveats). |
| `label` | `string` | `id` | Display label. |

Declaring `agentNode` also auto-declares the `__approvedTools` and `__approvalIds` channels
the governance path uses on resume.

### `toolNode(id, config)`

```ts
toolNode(id: string, config: ToolNodeConfig): GraphBuilder<TState & { messages: Message[] }>;
```

Add a tool node: it executes the tool calls emitted by the last AI message in the `messages`
channel (auto-declared as an append-reduced messages channel).

| `ToolNodeConfig` field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `tools` | `ToolRegistry` | — (required) | The tools to execute. |
| `parallel` | `boolean` | `false` | Run all tool calls concurrently instead of sequentially. |
| `label` | `string` | `id` | Display label. |

:::warning Approval-gated tool node on Rust
A tool node whose tool is `requiresApproval` **suspends** the run on the TS engine, but on the
Rust engine its handler throws a `DynamicInterrupt` that surfaces as a *node failure*, not a
clean suspension. Route such a graph with `ADRIANE_SDK_ENGINE=ts` if you need it. (Source:
`compiled-graph.ts`.)
:::

### `component(id, descriptor, options?)`

```ts
component(id: string, descriptor: ComponentDescriptor, options?: { label?: string }): this;
```

Add a **pure (no-LLM) compute node** from the [component catalog](/docs/reference/component-catalog).
The node carries the `{ kind, params }` carrier so it runs natively on the Rust engine, and
registers the descriptor's equivalent TS handler for the TS fallback path.

```ts
import { createGraph, components } from "@adriane-ai/graph-sdk";

createGraph({ name: "p" })
  .channel("name", { type: "string", default: "" })
  .channel("prompt", { type: "string", default: "" })
  .component("build", components.promptBuilder({ template: "Hi {{name}}", into: "prompt" }))
  .compile();
```

Expected result: a graph with one component node `build` that renders `Hi <name>` into the
`prompt` channel.

:::note Integration components are not `component(...)` nodes
`components.httpFetch` and `components.webSearch` return a plain `NodeHandler`, not a
`ComponentDescriptor`. Add them with `node(...)`, not `component(...)`. See the
[catalog](/docs/reference/component-catalog#integration-components-vendor-io).
:::

### `edge(from, to)`

```ts
edge(from: string, to: string): this;
```

Add an unconditional edge.

### `conditionalEdge(from, to, conditionName, predicate)`

```ts
conditionalEdge(
  from: string,
  to: string,
  conditionName: string,
  predicate: TypedCondition<TState>
): this;
```

Add a conditional edge guarded by a **named predicate**. The predicate is registered under
`conditionName` and evaluated against the live, typed state — Adriane never `eval`s a
user-supplied string, which is what keeps routing safe and inspectable (see the
[execution contract](/docs/core-concepts/execution-contract)).

```ts
.conditionalEdge("assistant", "review", "needsReview", (s) => s.channels.agentResult.requiresHumanReview)
```

### `entry(nodeId)`

```ts
entry(nodeId: string): this;
```

Override the entry node (which otherwise defaults to the first node added).

### `safeCompile()`

```ts
safeCompile(): Result<CompiledGraph<TState>, GraphCompileError>;
```

Validate and compile, returning a discriminated-union `Result` instead of throwing.
Validation runs through the Rust core when the native addon is present, else the TS
validator. See [errors](/docs/reference/errors#the-result-discriminated-union).

```ts
const result = createGraph({ name: "x" }).safeCompile();
if (!result.success) {
  console.error(result.error.errors); // GraphValidationError[]
} else {
  await result.data.run({});
}
```

Expected result: prints the validation errors (an empty graph has no entry node), since the
graph above declares no nodes.

### `compile()`

```ts
compile(): CompiledGraph<TState>;
```

Validate and compile into a runnable graph. Throws [`GraphCompileError`](/docs/reference/errors#graphcompileerror)
on validation failure. Equivalent to `safeCompile()` then throwing `result.error`.

## CompiledGraph methods

A validated, runnable graph. It holds the engine wiring (registries, checkpointer, event bus,
runtime) so callers never touch the lower-level `@adriane-ai/graph-runtime` primitives unless
they want to.

Execution runs on the **Rust engine** via `@adriane-ai/napi`. An in-process TypeScript runtime
backs development, tests, and platforms the native addon does not cover; the public API is
identical either way.

```mermaid
flowchart LR
  A[run / stream] -->|suspends| B[run_suspended]
  B --> C{gated for approval?}
  C -->|yes| D[approveAndResume]
  C -->|no| E[resume]
  D --> F[run_completed]
  E --> F
  A -->|no gate| F
```

### `run(initialData?, options?)`

```ts
run(
  initialData?: InitialData<TState>,
  options?: RunOptions
): Promise<TypedGraphState<TState>>;
```

Start a fresh run from the entry node and execute until completion or suspension. `options.runId`
lets you supply a stable run id to correlate with an external system; otherwise one is generated.

### `resume(runId)`

```ts
resume(runId: RunId): Promise<TypedGraphState<TState>>;
```

Resume a previously suspended run from its latest checkpoint.

:::warning Rust resume is instance-bound
On the Rust engine, `resume` / `approveAndResume` must follow a suspended run **on the same
`CompiledGraph` instance** — the suspended state is held in-process and fed back to Rust. A
fresh instance throws `No suspended state for run '...'`. Durable cross-process resume is the
control plane's job (with a `PgCheckpointer`). (Source: `compiled-graph.ts`.)
:::

### `approveAndResume(runId, options)`

```ts
approveAndResume(
  runId: RunId,
  options: ApproveAndResumeOptions
): Promise<TypedGraphState<TState>>;
```

Grant approval for the named tools and resume a run that suspended for approval. The agent
re-runs and executes the now-approved tools instead of gating them again. **An agent never
approves its own tools** — this is the human seam. Full loop in
[approval gates](/docs/governance/approval-gates).

| `ApproveAndResumeOptions` field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `approvedTools` | `string[]` | — (required) | Names of approval-gated tools the human grants; they execute on resume. |
| `resolvedBy` | `string` | `"human"` | The principal granting approval — never the requesting agent. The Rust engine rejects a resume where `resolvedBy` is empty or equals the tool's requester (the no-self-approval guard-rail). |

### `stream(initialData, mode, options?)`

```ts
stream(
  initialData: InitialData<TState>,
  mode: StreamMode,
  options?: RunOptions
): AsyncIterable<StreamEvent>;
```

Stream events as the graph executes. See [events and streams](/docs/reference/events-and-streams)
for the four `StreamMode` values and the `StreamEvent` union.

:::warning Single terminal event on Rust
The Rust engine has no incremental stream surface yet: when running on Rust, `stream` drives a
full run and yields a **single** terminal `state_value` event. Only the in-process TS engine
streams incrementally. (Source: `compiled-graph.ts`.)
:::

### `onEvent(handler)`

```ts
onEvent(handler: (event: RunEvent) => void): () => void;
```

Subscribe to the run-event lifecycle stream; returns an unsubscribe function. Events from
either engine arrive identically here — on the Rust path forwarded events are mirrored into the
same event bus. The full `RunEvent` union is in [events and streams](/docs/reference/events-and-streams).

### `usesRustEngine`

```ts
get usesRustEngine(): boolean;
```

`true` when this graph executes on the Rust engine, `false` on the TS fallback path. Use it to
branch on engine-specific behaviour (e.g. the streaming and resume caveats above).

### `definition` / `engine`

`definition` is the validated `GraphDefinition`. `engine` is an escape hatch to the underlying
TS `GraphRuntime` (time-travel, manual node execution); on the Rust path the runtime is present
but is **not** the executor, so branch on `usesRustEngine` first.

## Rust engine caveats

The public SDK API is identical across engines, but the `"auto"` engine policy (set via
`ADRIANE_SDK_ENGINE`, default `"auto"`) routes a few cases to TS to preserve semantics. All from
`compiled-graph.ts`:

- An agent node configured with a TS `approvalEngine` stays on the TS engine (the engine-backed
  approval flow is TS-only). `"rust"` overrides this.
- A handler that returns a routing `Command` (`{ goto }`) has its `goto` **dropped** on Rust
  (it applies a channel update + static-edge routing). Build a `conditionalEdge` instead.
- A `toolNode` with a `requiresApproval` tool *fails* rather than suspends on Rust (see the
  warning above).

Set `ADRIANE_SDK_ENGINE=ts` to force the TypeScript engine for these cases. The Rust engine is
the required production path; the TS engine is the dev/test/uncovered-platform path, not
deprecated.

## Next

- [Component catalog](/docs/reference/component-catalog)
- [Events and streams](/docs/reference/events-and-streams)
- [Errors](/docs/reference/errors)
