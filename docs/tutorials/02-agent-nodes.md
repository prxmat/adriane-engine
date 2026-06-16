# Tutorial 02 — Agent nodes

**Objective.** Add an LLM-driven **ReAct agent** to a graph with `agentNode`, read its
`AgentResult`, and run it fully offline with a deterministic mock LLM. Then see how the 16
**prebuilt micro-agents** give you a ready-made agent graph in one call.

Prerequisites: [Tutorial 01](./01-your-first-graph.md).

## What an agent node is

`agentNode` adds a node backed by a ReAct agent: the agent reasons, optionally calls tools,
and writes its result into an **output channel** (default `"agentResult"`, auto-declared and
added to the typed state). You drive it with an `LLMGateway`.

Offline, you use a `DefaultLLMGateway` plus a `MockLLMProviderAdapter` with a scripted
response — no API key, fully deterministic.

```ts
import {
  createGraph,
  DefaultLLMGateway,
  MockLLMProviderAdapter,
  type LLMGateway
} from "@adriane/graph-sdk";

// A mock LLM that returns a single final answer (no tool calls).
const mockLLM = (): LLMGateway => {
  const gateway = new DefaultLLMGateway();
  gateway.registerAdapter(
    new MockLLMProviderAdapter({
      provider: "anthropic",
      response: {
        content: "FINAL: The capital of France is Paris.",
        usage: { promptTokens: 0, completionTokens: 0 },
        model: "mock",
        provider: "anthropic"
      }
    })
  );
  return gateway;
};

const app = createGraph({ name: "qa" })
  .agentNode("assistant", {
    llm: mockLLM(),
    prompt: { system: "You are a concise assistant. Prefix your final answer with FINAL:." },
    maxIterations: 2
  })
  .compile();

const result = await app.run({});
console.log(result.status);                       // "completed"
console.log(result.channels.agentResult.reasoning); // the ReAct reasoning trace (mock content offline)
```

**Expected result:** the run completes and `channels.agentResult` holds the agent's
`AgentResult`.

## The `agentNode` config

`.agentNode(id, config)` accepts (the fields you'll reach for most):

| Field | Meaning |
| --- | --- |
| `llm` | The `LLMGateway` the agent runs on (a `DefaultLLMGateway` with an adapter). |
| `prompt` | `{ system: "..." }` inline, or `{ registry, id, version? }` from a `PromptRegistry`. |
| `tools` | A `ToolRegistry` the agent may call (see [Tutorial 03](./03-tools-and-tool-nodes.md)). |
| `tier` | A capability tier: `"frontier" \| "balanced" \| "fast" \| "creative"`. |
| `model` / `provider` | Pin a concrete model/provider (an explicit `model` always wins over `tier`). |
| `maxIterations` | Cap on the ReAct reasoning loop. |
| `suspendForApproval` | Suspend the whole run when a gated tool is reached (see [Tutorial 04](./04-human-approval-gates.md)). |
| `outputChannel` | Channel the result lands in (default `"agentResult"`). |

The result type is `AgentResult` (from `@adriane/agents-core`): `{ artifacts, blockers, approvalRequests, confidence, reasoning, requiresHumanReview }`. The output channel holds this **full object** (not a bare string) — you'll most often route on `confidence` or `requiresHumanReview`.

## Capability tiers, not hardcoded models

Instead of pinning a model, declare a **tier** and let the engine resolve it against whatever
providers are available in the environment:

```ts
createGraph({ name: "tiered" })
  .agentNode("writer", {
    llm: mockLLM(),
    prompt: { system: "Draft a short release note." },
    tier: "balanced"   // resolved to a concrete model by ModelPolicy (env-aware)
  })
  .compile();
```

- On the **Rust** path the bridge resolves the tier from the process env (e.g. with only
  `MISTRAL_API_KEY` set, every tier maps to the Mistral column).
- On the **TS** fallback path the SDK resolves it against `availableFromEnv()`.
- An explicit `model` (and `provider`) always overrides the tier.

> **Engine note.** On the Rust path the agent path builds its own LLM gateway from the
> environment (Mistral / Anthropic / Ollama / a deterministic mock). The TS
> `AgentNodeConfig.llm` you pass is used on the TS fallback path. The observable structure —
> final status, suspend-on-approval, approve-and-resume, lifecycle events — is identical across
> engines; only the mock's `reasoning` text differs.

## Routing on the agent's result

Because the result lands in a typed channel, you can route on it with a conditional edge — for
example, send a flagged answer to a human gate:

```ts
createGraph({ name: "reviewed-qa" })
  .channel("published", { type: "boolean", default: false })
  .agentNode("assistant", { llm: mockLLM(), prompt: { system: "Answer." } })
  .humanGate("review")
  .node("publish", async () => ({ published: true }))
  .conditionalEdge("assistant", "review", "needsReview", (s) => s.channels.agentResult.requiresHumanReview)
  .conditionalEdge("assistant", "publish", "isClean", (s) => !s.channels.agentResult.requiresHumanReview)
  .edge("review", "publish")
  .compile();
```

An agent **never approves its own output** — review is always a different principal. The full
governance loop is [Tutorial 04](./04-human-approval-gates.md).

## Prebuilt micro-agents

For common single-purpose agents you don't need to wire anything — `prebuilt` gives you a
ready-to-run `CompiledGraph`. Each runs on a deterministic mock gateway by default (no keys).

```ts
import { prebuilt } from "@adriane/graph-sdk";

const result = await prebuilt.summarizer().run({ question: "…a long text…" });
// The output channel holds the full `AgentResult` object — not a bare string.
// Offline, the default mock gateway makes the agent content literally "mock-response";
// wire a real provider (set a provider key in the env) for an actual summary.
const summary = result.channels.summary; // AgentResult
console.log(summary.reasoning);
```

The 16 prebuilt agents and the channel each writes:

| Agent | Tier | Output channel |
| --- | --- | --- |
| `summarizer` | fast | `summary` |
| `classifier` | fast | `label` |
| `extractor` | fast | `extracted` |
| `translator` | fast | `translation` |
| `sentimentAnalyzer` | fast | `sentiment` |
| `entityExtractor` | fast | `entities` |
| `piiRedactor` | fast | `redacted` |
| `intentClassifier` | fast | `intent` |
| `titleGenerator` | fast | `title` |
| `keywordExtractor` | fast | `keywords` |
| `sqlGenerator` | balanced | `sql` |
| `questionAnswerer` | balanced | `answer` |
| `ragAnswerer` | balanced | `answer` |
| `refundApprover` | balanced | `refundDecision` |
| `codeReviewer` | frontier | `review` |
| `copyEditor` | creative | `edited` |

Override the gateway, model or tier per call:

```ts
prebuilt.classifier({ tierOverride: "balanced" });   // change the tier
prebuilt.summarizer({ model: "claude-opus-4-8" });   // pin a model
prebuilt.questionAnswerer({ llm: myGateway });        // run on a real gateway
```

`ragAnswerer` is special — it's a composed graph (`retriever` + `reranker` components + an
agent step) and accepts extra options like `docs`, `k`, and `questionChannel`:

```ts
await prebuilt.ragAnswerer({
  docs: [{ id: "d1", content: "Adriane checkpoints after every node." }],
  k: 3
}).run({ question: "How does Adriane stay resumable?" });
```

## Try it

```bash
pnpm --filter @adriane/graph-sdk example:agent   # examples/agent.ts — agent + approval gate
```

## Next

[Tutorial 03 — Tools and tool nodes](./03-tools-and-tool-nodes.md): give your agent tools, and
run tool calls in a dedicated node.
