# Tutorial 04 — Human-approval gates

**Objective.** Put a human in the loop. You'll learn the two governance seams Adriane offers:

1. A **structural `humanGate` node** that suspends the run until a human approves and you
   `resume()` it.
2. **Agent-native tool approval** (`suspendForApproval`) — an agent reaches for a sensitive
   tool, the run suspends cleanly, and you `approveAndResume()` to grant the tool and continue.

An agent **never approves its own output** — approval is always a different principal. This is
the core loop Adriane is built around.

Prerequisites: [Tutorial 02](./02-agent-nodes.md), [Tutorial 03](./03-tools-and-tool-nodes.md).

## Seam 1 — a structural human gate

`.humanGate(id)` adds a node that **suspends the run cleanly** when execution reaches it. You
resume from the latest checkpoint after a human approves out of band.

```ts
import { createGraph } from "@adriane/graph-sdk";

const app = createGraph({ name: "publish-flow" })
  .channel("draft", { type: "string", default: "" })
  .channel("approved", { type: "boolean", default: false })
  .node("write", async () => ({ draft: "Hello from Adriane." }))
  .humanGate("review")                            // suspends here
  .node("publish", async () => ({ approved: true }))
  .edge("write", "review")
  .edge("review", "publish")
  .compile();

// 1) Start the run — it suspends at the human gate.
const suspended = await app.run();
console.log(suspended.status);        // "suspended"
console.log(suspended.currentNodeId); // "review"

// 2) A human approves out-of-band, then you resume from the latest checkpoint.
const done = await app.resume(suspended.runId);
console.log(done.status);             // "completed"
console.log(done.channels.approved);  // true
```

**Expected result:** the first call returns `status: "suspended"` paused at `review`; after a
human approves, `resume(runId)` advances past the gate and completes, setting `approved` to
`true`. This is the shipped `examples/quickstart.ts`.

> **Engine note (Rust path).** `resume()` / `approveAndResume()` must follow a suspended run
> **on the same `CompiledGraph` instance**, which holds the suspended state to feed back to the
> Rust engine. (On the TS engine, the in-memory checkpointer holds it too.)

## Seam 2 — agent-native tool approval

When an agent reaches for a tool marked `requiresApproval`, set `suspendForApproval: true` on
the agent node and the **whole run suspends** the moment approval is needed — the tool does
**not** run. You then grant the named tools and resume with `approveAndResume`.

```ts
import {
  createGraph,
  DefaultLLMGateway,
  InMemoryToolRegistry,
  MockLLMProviderAdapter,
  type LLMGateway,
  type ToolId
} from "@adriane/graph-sdk";

// A mock LLM that always asks to call the `refund` tool.
const mockLLM = (toolName: string): LLMGateway => {
  const gateway = new DefaultLLMGateway();
  gateway.registerAdapter(
    new MockLLMProviderAdapter({
      provider: "anthropic",
      response: {
        content: "",
        toolCalls: [{ id: "tu1", name: toolName, input: {} }],
        stopReason: "tool_use",
        usage: { promptTokens: 0, completionTokens: 0 },
        model: "mock",
        provider: "anthropic"
      }
    })
  );
  return gateway;
};

const tools = new InMemoryToolRegistry();
tools.register(
  {
    id: "refund" as ToolId,
    name: "refund",
    description: "Issues a customer refund. Sensitive.",
    inputSchema: { parse: (v: unknown) => v },
    outputSchema: { parse: (v: unknown) => v },
    permissions: ["payments:write"],
    requiresApproval: true,                 // gated
    jsonSchema: { type: "object" }
  },
  async () => ({ ok: true })
);

const app = createGraph({ name: "support-agent" })
  .agentNode("assistant", {
    llm: mockLLM("refund"),
    prompt: { system: "You are a support agent. Use tools when needed." },
    tools,
    suspendForApproval: true,
    maxIterations: 2
  })
  .compile();

// 1) The agent reaches for `refund` → run suspends for approval (tool NOT executed).
const suspended = await app.run();
console.log(suspended.status);                                   // "suspended"
console.log(suspended.currentNodeId);                            // "assistant"
console.log(suspended.channels.agentResult?.approvalRequests.length); // 1

// 2) A human grants approval; the run resumes and the tool runs.
const done = await app.approveAndResume(suspended.runId, { approvedTools: ["refund"] });
console.log(done.status);                                        // "completed"
```

**Expected result:** the run suspends at the agent node with one `approvalRequest` recorded and
the tool unexecuted; after `approveAndResume(runId, { approvedTools: ["refund"] })` the agent
re-runs, executes the now-approved tool, and the run completes. This is the shipped
`examples/agent.ts`.

### How `approveAndResume` works

`approveAndResume(runId, { approvedTools })` records the approved tool **names** and resumes:

- On the **Rust** path the engine writes the approved tools into the reserved `__approvedTools`
  channel before resuming.
- On the **TS** path the channel is updated directly.

Either way the agent re-runs and executes exactly the tools you approved — it never gates them
again, and never approves anything itself.

### The mock across suspend/resume

The scripted mock gateway is **stateful across suspend/resume**: a resumed agent re-runs and
consumes the *next* scripted response. So if you script the gated tool call to execute after
approval, script that tool turn **twice in a row** (once before suspension, once after resume),
then the `FINAL:` turn. The shipped examples follow this convention.

## Choosing a seam

- Use a **`humanGate`** for a *structural* checkpoint in the flow ("a person must look at this
  step before we continue"), e.g. publishing, deploying, or routing a low-confidence answer.
- Use **`suspendForApproval`** when an *agent* might reach for a sensitive tool and you want the
  agent itself to be unable to proceed without human sign-off on that specific action.

Both suspend cleanly and resume from the latest checkpoint — see
[Tutorial 05](./05-checkpointing-and-resume.md).

> **TS-only: `ApprovalEngine`.** For a durable, out-of-band approval workflow you can pass an
> `approvalEngine` to `agentNode`; on suspend it files one request per gated tool, and on
> resume it executes the tools the engine reports as approved. This engine-backed flow runs on
> the **TypeScript engine** (a graph using it stays on TS under the default `auto` policy). The
> shipped `examples/startup-e2e.ts` demonstrates it end to end.

## Try it

```bash
pnpm --filter @adriane/graph-sdk example         # examples/quickstart.ts — humanGate + resume
pnpm --filter @adriane/graph-sdk example:agent   # examples/agent.ts — suspendForApproval + approveAndResume
```

## Next

[Tutorial 05 — Checkpointing and resume](./05-checkpointing-and-resume.md): the contract that
makes all of this reliable.
