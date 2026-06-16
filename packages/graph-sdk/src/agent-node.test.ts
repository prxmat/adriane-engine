import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
// Import the in-memory engine directly (not the package index) so the test never
// pulls the Pg engine and its `db`/`pg` dependency chain.
import { InMemoryApprovalEngine } from "../../approval-engine/src/in-memory-approval-engine.js";

import {
  createGraph,
  DefaultLLMGateway,
  InMemoryToolRegistry,
  MockLLMProviderAdapter,
  streamAgentTokens,
  type AgentResult,
  type LLMGateway,
  type Message,
  type ToolId
} from "./index.js";

const mockGateway = (content: string): LLMGateway => {
  const gateway = new DefaultLLMGateway();
  gateway.registerAdapter(
    new MockLLMProviderAdapter({
      provider: "anthropic",
      response: {
        content,
        usage: { promptTokens: 0, completionTokens: 0 },
        model: "mock",
        provider: "anthropic"
      }
    })
  );
  return gateway;
};

const passthrough = { parse: (value: unknown) => value };

describe("@adriane/graph-sdk agent node", () => {
  // This block unit-tests the **TS agent-node handler + the TS `AgentNodeConfig.llm`
  // gateway**, asserting the gateway's exact output text flows into `AgentResult`.
  // Under `auto`, agent nodes now route to the Rust engine, which builds its *own*
  // gateway (env adapters or a deterministic mock) and so emits different text — the
  // documented gateway boundary. We pin these to the TS engine so they keep testing
  // the TS path specifically; the cross-engine *structural* equivalence (status,
  // suspend/approve, routing, events) is covered by `rust-engine.test.ts`.
  let savedEngine: string | undefined;
  beforeEach(() => {
    savedEngine = process.env.ADRIANE_SDK_ENGINE;
    process.env.ADRIANE_SDK_ENGINE = "ts";
  });
  afterEach(() => {
    if (savedEngine === undefined) {
      delete process.env.ADRIANE_SDK_ENGINE;
    } else {
      process.env.ADRIANE_SDK_ENGINE = savedEngine;
    }
  });

  it("runs a ReAct agent and writes its result to the output channel", async () => {
    const app = createGraph({ name: "assistant-graph" })
      .agentNode("assistant", {
        llm: mockGateway("FINAL: all done"),
        prompt: { system: "You are a helpful assistant." },
        maxIterations: 2
      })
      .compile();

    const result = await app.run({ question: "hi" });
    const agentResult = (result.channels as Record<string, AgentResult>).agentResult;

    expect(result.status).toBe("completed");
    expect(agentResult?.requiresHumanReview).toBe(false);
    expect(agentResult?.reasoning).toContain("FINAL: all done");
  });

  it("flags requiresHumanReview when the agent reaches for an approval-gated tool", async () => {
    const tools = new InMemoryToolRegistry();
    tools.register(
      {
        id: "wire_transfer" as ToolId,
        name: "wire_transfer",
        description: "Moves money. Sensitive.",
        inputSchema: passthrough,
        outputSchema: passthrough,
        permissions: ["payments:write"],
        requiresApproval: true,
        jsonSchema: { type: "object" }
      },
      async () => ({ ok: true })
    );

    const app = createGraph({ name: "governed-graph" })
      .agentNode("assistant", {
        llm: mockGateway("ACTION: wire_transfer {}"),
        prompt: { system: "Use tools when needed." },
        tools,
        maxIterations: 2
      })
      .compile();

    const result = await app.run();
    const agentResult = (result.channels as Record<string, AgentResult>).agentResult;

    // The agent never self-approves: it surfaces an approval request instead.
    expect(agentResult?.requiresHumanReview).toBe(true);
    expect(agentResult?.approvalRequests.length).toBe(1);
  });

  it("routes a review-required agent into a human gate (suspends the run)", async () => {
    const tools = new InMemoryToolRegistry();
    tools.register(
      {
        id: "delete_account" as ToolId,
        name: "delete_account",
        description: "Destructive.",
        inputSchema: passthrough,
        outputSchema: passthrough,
        permissions: ["admin"],
        requiresApproval: true,
        jsonSchema: { type: "object" }
      },
      async () => ({ ok: true })
    );

    const needsReview = (channels: Record<string, unknown>): boolean =>
      Boolean((channels.agentResult as AgentResult | undefined)?.requiresHumanReview);

    const app = createGraph({ name: "approval-routing" })
      .agentNode("assistant", {
        llm: mockGateway("ACTION: delete_account {}"),
        prompt: { system: "Use tools when needed." },
        tools,
        maxIterations: 2
      })
      .humanGate("human-review")
      .node("execute", async () => ({ executed: true }))
      .conditionalEdge("assistant", "human-review", "needsReview", (s) => needsReview(s.channels))
      .conditionalEdge("assistant", "execute", "autoApproved", (s) => !needsReview(s.channels))
      .compile();

    const result = await app.run();
    expect(result.status).toBe("suspended");
    expect(result.currentNodeId).toBe("human-review");
  });
});

describe("@adriane/graph-sdk agent node — native suspend on approval", () => {
  const toolCallGateway = (toolName: string): LLMGateway => {
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

  it("suspends the run for approval, then executes the tool once granted on resume", async () => {
    const handler = vi.fn(async () => ({ ok: true }));
    const tools = new InMemoryToolRegistry();
    tools.register(
      {
        id: "refund" as ToolId,
        name: "refund",
        description: "Issues a refund.",
        inputSchema: passthrough,
        outputSchema: passthrough,
        permissions: ["payments:write"],
        requiresApproval: true,
        jsonSchema: { type: "object" }
      },
      handler
    );

    const app = createGraph({ name: "native-approval" })
      .agentNode("assistant", {
        llm: toolCallGateway("refund"),
        prompt: { system: "Use tools when needed." },
        tools,
        suspendForApproval: true,
        maxIterations: 2
      })
      .compile();

    const suspended = await app.run({}, { runId: "run_appr_1" as never });
    expect(suspended.status).toBe("suspended");
    expect(handler).not.toHaveBeenCalled(); // gated before execution

    const done = await app.approveAndResume(suspended.runId, { approvedTools: ["refund"] });
    expect(done.status).toBe("completed");
    expect(handler).toHaveBeenCalled(); // ran once approval was granted
  });

  it("routes approval through an ApprovalEngine and resumes after a human approves", async () => {
    const engine = new InMemoryApprovalEngine();
    const handler = vi.fn(async () => ({ ok: true }));
    const tools = new InMemoryToolRegistry();
    tools.register(
      {
        id: "refund" as ToolId,
        name: "refund",
        description: "Issues a refund.",
        inputSchema: passthrough,
        outputSchema: passthrough,
        permissions: ["payments:write"],
        requiresApproval: true,
        jsonSchema: { type: "object" }
      },
      handler
    );

    const app = createGraph({ name: "engine-approval" })
      .agentNode("assistant", {
        llm: toolCallGateway("refund"),
        prompt: { system: "Use tools when needed." },
        tools,
        suspendForApproval: true,
        approvalEngine: engine,
        maxIterations: 2
      })
      .compile();

    const suspended = await app.run({}, { runId: "run_eng_1" as never });
    expect(suspended.status).toBe("suspended");
    expect(handler).not.toHaveBeenCalled();

    // The engine now holds a pending request filed by the agent.
    const pending = await engine.getPending(suspended.runId);
    expect(pending).toHaveLength(1);
    expect(pending[0]?.requestedBy).toBe("assistant");

    // A human (a different principal) approves it — the engine forbids self-approval.
    await engine.approve(pending[0]!.id, "alice");

    const done = await app.resume(suspended.runId);
    expect(done.status).toBe("completed");
    expect(handler).toHaveBeenCalled(); // executed once the engine reported approval
  });
});

describe("@adriane/graph-sdk agent node — native tool-calling", () => {
  // Asserts the exact TS-gateway tool_use → observation → final trace text, so it
  // pins to the TS engine (the Rust path uses its own gateway; see the note above).
  let savedEngine: string | undefined;
  beforeEach(() => {
    savedEngine = process.env.ADRIANE_SDK_ENGINE;
    process.env.ADRIANE_SDK_ENGINE = "ts";
  });
  afterEach(() => {
    if (savedEngine === undefined) {
      delete process.env.ADRIANE_SDK_ENGINE;
    } else {
      process.env.ADRIANE_SDK_ENGINE = savedEngine;
    }
  });

  it("executes a tool from structured tool_use, then finalizes (end to end)", async () => {
    const gateway = new DefaultLLMGateway();
    gateway.registerAdapter(
      new MockLLMProviderAdapter({
        provider: "anthropic",
        responses: [
          {
            content: "",
            toolCalls: [{ id: "tu1", name: "lookup", input: { id: 42 } }],
            stopReason: "tool_use",
            usage: { promptTokens: 0, completionTokens: 0 },
            model: "mock",
            provider: "anthropic"
          },
          {
            content: "FINAL: found it",
            usage: { promptTokens: 0, completionTokens: 0 },
            model: "mock",
            provider: "anthropic"
          }
        ]
      })
    );

    const tools = new InMemoryToolRegistry();
    tools.register(
      {
        id: "lookup" as ToolId,
        name: "lookup",
        description: "Looks something up.",
        inputSchema: passthrough,
        outputSchema: passthrough,
        permissions: [],
        jsonSchema: { type: "object" }
      },
      async () => ({ found: true })
    );

    const app = createGraph({ name: "native-tools" })
      .agentNode("assistant", { llm: gateway, prompt: { system: "Use tools." }, tools, maxIterations: 4 })
      .compile();

    const result = await app.run();
    const agentResult = (result.channels as Record<string, AgentResult>).agentResult;

    expect(result.status).toBe("completed");
    expect(agentResult?.requiresHumanReview).toBe(false);
    // The tool ran via the structured tool_use path; its observation is in the trace.
    expect(agentResult?.reasoning).toContain('observation:{"found":true}');
  });
});

describe("@adriane/graph-sdk tool node", () => {
  it("executes the tool calls emitted by the last AI message", async () => {
    const tools = new InMemoryToolRegistry();
    tools.register(
      {
        id: "echo" as ToolId,
        name: "echo",
        description: "Echoes its input.",
        inputSchema: passthrough,
        outputSchema: passthrough,
        permissions: []
      },
      async (input) => ({ echoed: input })
    );

    const app = createGraph({ name: "tool-graph" }).toolNode("tools", { tools }).compile();

    const aiMessage: Message = {
      id: "m1" as Message["id"],
      role: "ai",
      content: "",
      toolCalls: [{ id: "call_1", name: "echo", input: { hello: "world" } }],
      createdAt: new Date()
    };

    const result = await app.run({ messages: [aiMessage] });
    const messages = (result.channels as Record<string, unknown[]>).messages ?? [];

    expect(result.status).toBe("completed");
    expect(messages.length).toBe(2); // original AI message + tool result
    const toolMessage = messages.at(-1) as { role: string; content: string };
    expect(toolMessage.role).toBe("tool");
    expect(toolMessage.content).toContain("world");
  });
});

describe("@adriane/graph-sdk streamAgentTokens", () => {
  it("streams the agent's reply token by token via the gateway stream", async () => {
    const gateway = new DefaultLLMGateway();
    gateway.registerAdapter(
      new MockLLMProviderAdapter({
        provider: "anthropic",
        chunks: [
          { delta: "Bon", done: false },
          { delta: "jour", done: false },
          { delta: " !", done: false },
          { delta: "", done: true }
        ]
      })
    );

    const deltas: string[] = [];
    for await (const delta of streamAgentTokens({ llm: gateway, prompt: { system: "Sois bref." } }, "Salut")) {
      deltas.push(delta);
    }

    expect(deltas).toEqual(["Bon", "jour", " !"]); // empty terminal delta is dropped
    expect(deltas.join("")).toBe("Bonjour !");
  });
});
