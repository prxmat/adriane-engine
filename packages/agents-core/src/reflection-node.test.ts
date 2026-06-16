import { describe, expect, it } from "vitest";
import { DefaultLLMGateway, MockLLMProviderAdapter } from "../../llm-gateway/src/index.js";

import { createReflectionNode } from "./reflection-node.js";

describe("ReflectionNode", () => {
  it("triggers retry command when critique detects issue", async () => {
    const gateway = new DefaultLLMGateway();
    gateway.registerAdapter(
      new MockLLMProviderAdapter({
        provider: "openai",
        response: {
          content: "problem detected, retry",
          usage: { promptTokens: 1, completionTokens: 1 },
          model: "mock",
          provider: "openai"
        }
      })
    );
    const node = createReflectionNode({
      llm: gateway,
      previousNodeId: "prev" as never,
      maxReflections: 2
    });
    const result = await node({}, {} as never, { memory: {} as never });
    expect((result as { goto?: unknown }).goto).toBe("prev");
  });
});
