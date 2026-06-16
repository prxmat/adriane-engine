import { describe, expect, it } from "vitest";
import { DefaultLLMGateway, MockLLMProviderAdapter } from "../../llm-gateway/src/index.js";

import { compressShortTerm } from "./working-memory.js";

describe("working memory", () => {
  it("compresses old short-term messages when token limit exceeded", async () => {
    const gateway = new DefaultLLMGateway();
    gateway.registerAdapter(
      new MockLLMProviderAdapter({
        provider: "openai",
        response: {
          content: "summary",
          usage: { promptTokens: 1, completionTokens: 1 },
          model: "mock",
          provider: "openai"
        }
      })
    );
    const compressed = await compressShortTerm(
      [
        { id: "1" as never, role: "human", content: "a".repeat(80), createdAt: new Date() },
        { id: "2" as never, role: "ai", content: "b".repeat(80), createdAt: new Date() },
        { id: "3" as never, role: "tool", content: "ok", toolCallId: "t1", createdAt: new Date() }
      ],
      gateway,
      20
    );
    expect(compressed.length).toBeGreaterThan(0);
    expect(compressed[0]?.role).toBe("system");
  });
});
