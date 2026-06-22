import { describe, expect, it, vi } from "vitest";

import {
  createGraph,
  DefaultLLMGateway,
  InMemoryToolRegistry,
  MockLLMProviderAdapter,
  streamAgentTokens,
  type LLMGateway,
  type Message,
  type ToolId
} from "./index.js";

// NOTE: the SDK runs **exclusively on the Rust engine** (the TS fallback was removed —
// see `RustEngineRequiredError`). Agent nodes execute natively on Rust, which builds its
// own gateway from `provider`/`model` + env; a TS `AgentNodeConfig.llm` is not consulted
// on the run path. So this file no longer pins `ADRIANE_SDK_ENGINE=ts` to assert exact
// TS-gateway output text. The agent's *structural* governance contract — runs to
// completion, suspends on an approval-gated tool, resumes via `approveAndResume`,
// routes conditional edges — is covered on the Rust engine by `rust-engine.test.ts`.

const passthrough = { parse: (value: unknown) => value };

describe("@adriane-ai/graph-sdk agent node — suspend on approval (channel-based)", () => {
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
});

describe("@adriane-ai/graph-sdk tool node", () => {
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

describe("@adriane-ai/graph-sdk streamAgentTokens", () => {
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
