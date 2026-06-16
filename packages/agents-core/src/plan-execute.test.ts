import { describe, expect, it } from "vitest";
import { DefaultLLMGateway, MockLLMProviderAdapter } from "../../llm-gateway/src/index.js";
import { InMemoryStore } from "../../memory-store/src/in-memory-store.js";

import { ExecutorAgent, PlannerAgent } from "./plan-execute.js";
import type { WorkingMemory } from "./working-memory.js";

describe("Plan/Execute agents", () => {
  it("generates a plan and executes it sequentially", async () => {
    const gateway = new DefaultLLMGateway();
    gateway.registerAdapter(
      new MockLLMProviderAdapter({
        provider: "openai",
        response: {
          content: "Step 1\nStep 2",
          usage: { promptTokens: 1, completionTokens: 1 },
          model: "mock",
          provider: "openai"
        }
      })
    );
    const planner = new PlannerAgent(gateway);
    const memory = new InMemoryStore();
    const workingMemory: WorkingMemory = { shortTerm: [], longTerm: memory };
    const planned = await planner.run({ objective: "Do X" }, { runId: "run1" } as never, { memory, workingMemory });
    const steps = JSON.parse(planned.reasoning) as Array<{ id: string; text: string }>;

    const executor = new ExecutorAgent(async (step) => `done:${step.text}`);
    const executed = await executor.run({ steps }, { runId: "run1" } as never, { memory, workingMemory });
    expect(executed.reasoning).toContain("done:Step 1");
    expect(executed.reasoning).toContain("done:Step 2");
  });
});
