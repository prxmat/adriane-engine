# Tutorial 03 — Tools and tool nodes

**Objective.** Register tools an agent can call, run an agent that calls them, and learn the
two ways to execute tool calls: **inside the agent node** (via its `tools` registry) and in a
**dedicated `toolNode`** that executes the tool calls from the last AI message. Then meet the
pure, no-LLM **components** for compute steps that don't need a model at all.

Prerequisites: [Tutorial 02](./02-agent-nodes.md).

## Registering tools

Tools live in an `InMemoryToolRegistry`. Each tool has a definition plus a handler:

```ts
import { InMemoryToolRegistry, type ToolId } from "@adriane/graph-sdk";

const tools = new InMemoryToolRegistry();
const passthrough = { parse: (value: unknown) => value };

tools.register(
  {
    id: "search_documents" as ToolId,
    name: "search_documents",
    description: "Keyword search over the corpus. Returns the top documents with scores.",
    inputSchema: passthrough,
    outputSchema: passthrough,
    permissions: [],
    jsonSchema: {
      type: "object",
      properties: { query: { type: "string" } },
      required: ["query"]
    }
  },
  async (input: unknown) => {
    const { query } = input as { query: string };
    // ... do the search ...
    return { hits: [{ id: "checkpointing", score: 3 }] };
  }
);
```

The definition fields:

- `id`, `name`, `description` — identity and the description the agent sees.
- `inputSchema` / `outputSchema` — parsers (anything with `.parse(value)`).
- `jsonSchema` — the JSON schema the LLM is shown for the tool's arguments.
- `permissions` — declared scopes (e.g. `["payments:write"]`).
- `requiresApproval` — when `true`, the tool is gated behind human approval (see
  [Tutorial 04](./04-human-approval-gates.md)).

## An agent that calls tools

Hand the registry to `agentNode` via `tools`. The ReAct loop will emit tool calls, the node
executes them, feeds results back, and iterates up to `maxIterations`. Offline, you script the
LLM's tool-use turns with a `responses` array (one element per turn):

```ts
import {
  createGraph,
  DefaultLLMGateway,
  InMemoryToolRegistry,
  MockLLMProviderAdapter,
  type LLMGateway,
  type LLMResponse,
  type ToolId
} from "@adriane/graph-sdk";

const toolTurn = (name: string, input: Record<string, unknown>): LLMResponse => ({
  content: "",
  toolCalls: [{ id: `tu_${name}`, name, input }],
  stopReason: "tool_use",
  usage: { promptTokens: 0, completionTokens: 0 },
  model: "mock",
  provider: "anthropic"
});

const finalTurn = (content: string): LLMResponse => ({
  content,
  usage: { promptTokens: 0, completionTokens: 0 },
  model: "mock",
  provider: "anthropic"
});

const scripted = (responses: LLMResponse[]): LLMGateway => {
  const gateway = new DefaultLLMGateway();
  gateway.registerAdapter(new MockLLMProviderAdapter({ provider: "anthropic", responses }));
  return gateway;
};

const tools = new InMemoryToolRegistry();
const passthrough = { parse: (v: unknown) => v };
tools.register(
  {
    id: "search_documents" as ToolId,
    name: "search_documents",
    description: "Keyword search over the corpus.",
    inputSchema: passthrough,
    outputSchema: passthrough,
    permissions: [],
    jsonSchema: { type: "object", properties: { query: { type: "string" } }, required: ["query"] }
  },
  async () => ({ hits: [{ id: "checkpointing", score: 3 }] })
);

const app = createGraph({ name: "tool-using-agent" })
  .agentNode("assistant", {
    llm: scripted([
      toolTurn("search_documents", { query: "resume after crash" }),  // turn 1: call the tool
      finalTurn("FINAL: Adriane resumes from the latest checkpoint.")  // turn 2: final answer
    ]),
    prompt: { system: "Use the search tool, then answer." },
    tools,
    maxIterations: 5
  })
  .compile();

const result = await app.run({});
console.log(result.status); // "completed"
```

**Expected result:** the agent calls `search_documents` once, then produces its final answer;
the run completes. This is the exact pattern the shipped `examples/qa-rag.ts` uses.

> **Mock-sequencing rule.** The scripted gateway is **stateful** across turns (and across
> suspend/resume). Each agent turn consumes the next scripted response. Order them: tool-use
> turn(s) first, then the `FINAL:` turn.

## A dedicated tool node

Sometimes you want a separate node that just executes the tool calls emitted by the last AI
message in the `messages` channel — for example, to run them in parallel or to make the
tool-execution step explicit in the graph. That's `toolNode`:

```ts
import { createGraph } from "@adriane/graph-sdk";

createGraph({ name: "agent-then-tools" })
  .messagesChannel()                              // append-reduced "messages" channel
  .agentNode("plan", { llm: scripted([/* … */]), prompt: { system: "Plan tool calls." }, tools })
  .toolNode("run-tools", { tools, parallel: true }) // execute the tool calls; optionally in parallel
  .edge("plan", "run-tools")
  .compile();
```

`.toolNode(id, { tools, parallel? })` executes the tool calls from the last AI message in
`messages` (auto-declared as an append-reduced channel). A tool flagged `requiresApproval`
suspends the run via a dynamic interrupt instead of executing.

## Components: pure compute, no LLM

Not every step needs an agent. Adriane ships **30 components** — pure, deterministic compute
building blocks addressed by a `kind` and `params`. Add one with `.component(id, descriptor)`,
where the descriptor comes from a `components.*` factory. They run natively on the Rust engine
with a faithful TypeScript fallback.

```ts
import { createGraph, components } from "@adriane/graph-sdk";

const app = createGraph({ name: "prep" })
  .channel("name", { type: "string", default: "" })
  .channel("prompt", { type: "string", default: "" })
  // Render {{name}} from the channels into the `prompt` channel.
  .component("build", components.promptBuilder({ template: "Hi {{name}}!", into: "prompt" }))
  .compile();

const out = await app.run({ name: "Ada" });
console.log(out.channels.prompt); // "Hi Ada!"
```

A few you'll reach for often (all pure, with their real params):

| Component | Factory | Key params |
| --- | --- | --- |
| Prompt builder | `components.promptBuilder({ template, into })` | render `{{var}}` placeholders into a channel |
| Text cleaner | `components.textCleaner({ from, into, stripHtml?, lowercase?, collapseWhitespace?, trim? })` | normalise text |
| Document splitter | `components.documentSplitter({ from, into, by: "chars" \| "sentences", size, overlap? })` | chunk text |
| Retriever | `components.retriever({ query, into, k?, docs })` | mock-embedding top-`k` over a corpus |
| Reranker | `components.reranker({ from, into, query? })` | reorder a results array |

The full set of 30 kinds (e.g. `jsonValidator`, `outputParser`, `router`, `conditionalRouter`,
`csvParser`, `htmlToText`, `regexExtractor`, `answerBuilder`, `fieldMapper`, `fieldExtractor`,
`bm25Retriever`, `keywordRetriever`, `sentenceWindowSplitter`, `languageDetector`,
`metadataFilter`, `listJoiner`, `mergeRanker`, `evaluator`, `chatMessageBuilder`,
`documentWriter`, `deduplicator`, `truncator`, `documentJoiner`) lives in
`packages/graph-sdk/src/components.ts`. Each factory's params are typed there.

> **Router safety.** `router` and `conditionalRouter` are **pure predicates**, not eval'd
> code — same safety guarantee as conditional edges.

### Vendor-I/O components

Two components — `httpFetch` and `webSearch` — are **integrations**, not pure Rust components.
They return a plain node handler (a closure over an injected I/O impl) and are added with
`.node(...)`, not `.component(...)`. Inject a fake impl to keep tests offline:

```ts
createGraph({ name: "fetch" })
  .channel("body", { type: "json", default: null })
  .node("get", components.httpFetch({ url: "https://example.com", into: "body", fetchImpl: fakeFetch }));
```

## Real-data retrieval (beyond the mock)

For real embeddings-backed retrieval, use `semanticRetriever` with `createEmbeddings` and
`createVectorStore` — covered in [Tutorial 02's prebuilt `ragAnswerer`](./02-agent-nodes.md)
and demonstrated in `examples/product-pipeline.ts` and `examples/doc-qa-reference.ts`.

## Try it

```bash
pnpm --filter @adriane/graph-sdk example:qa     # examples/qa-rag.ts — agent + search/fetch tools
pnpm --filter @adriane/graph-sdk example:docqa  # examples/doc-qa-reference.ts — components + agent
```

## Next

[Tutorial 04 — Human-approval gates](./04-human-approval-gates.md): suspend a run for human
approval and resume it.
