import { describe, expect, it } from "vitest";
import { DefaultLLMGateway, MockLLMProviderAdapter } from "../../llm-gateway/src/index.js";

import { SupervisorAgent } from "./supervisor.js";

describe("SupervisorAgent", () => {
  it("chooses the expected agent", async () => {
    const gateway = new DefaultLLMGateway();
    gateway.registerAdapter(
      new MockLLMProviderAdapter({
        provider: "openai",
        response: {
          content: "AGENT:agent-b",
          usage: { promptTokens: 1, completionTokens: 1 },
          model: "mock",
          provider: "openai"
        }
      })
    );
    const supervisor = new SupervisorAgent({
      id: "supervisor" as never,
      name: "Supervisor",
      description: "Routes to sub-agents",
      llm: gateway,
      config: { agents: ["agent-a" as never, "agent-b" as never], maxRounds: 3 },
      agentNodeMap: { "agent-a": "node-a" as never, "agent-b": "node-b" as never },
      agentDescriptions: { "agent-a": "Alpha", "agent-b": "Beta" }
    });
    const command = await supervisor.nextCommand({ objective: "Solve issue" }, { channels: {} } as never);
    expect(command).not.toBe("FINISH");
    expect((command as { goto: string }).goto).toBe("node-b");
  });
});
