import { describe, expect, it } from "vitest";

import { AgentCoordinator } from "./coordination.js";

describe("AgentCoordinator", () => {
  it("aggregates parallel agent results and detects conflicts", async () => {
    const coordinator = new AgentCoordinator();
    const mkAgent = (reasoning: string, confidence: number) =>
      ({
        id: reasoning as never,
        name: reasoning,
        description: reasoning,
        run: async () => ({
          artifacts: [],
          blockers: [],
          approvalRequests: [],
          confidence,
          reasoning,
          requiresHumanReview: false
        })
      }) as never;

    const result = await coordinator.runParallel(
      [
        { agent: mkAgent("A", 0.8), input: {}, proposedUpdate: { shared: 1, a: true } },
        { agent: mkAgent("B", 0.6), input: {}, proposedUpdate: { shared: 2, b: true } }
      ],
      {} as never,
      {} as never
    );
    expect(result.reasoning).toContain("A");
    expect(result.reasoning).toContain("B");
    expect(result.conflicts).toContain("shared");
    expect(result.channelUpdates.a).toBe(true);
    expect(result.channelUpdates.b).toBe(true);
  });
});
