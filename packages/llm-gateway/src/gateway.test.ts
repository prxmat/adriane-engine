import { describe, expect, it } from "vitest";

import { LLMProviderNotFoundError, LLMValidationError } from "./errors.js";
import { DefaultLLMGateway } from "./gateway.js";
import { MockLLMProviderAdapter } from "./mock-adapter.js";

describe("DefaultLLMGateway", () => {
  it("routes complete request to the correct adapter", async () => {
    const gateway = new DefaultLLMGateway();
    gateway.registerAdapter(
      new MockLLMProviderAdapter({
        provider: "openai",
        response: {
          content: "openai-response",
          usage: { promptTokens: 10, completionTokens: 20 },
          model: "gpt-test",
          provider: "openai"
        }
      })
    );

    const response = await gateway.complete({
      provider: "openai",
      model: "gpt-test",
      messages: [{ role: "user", content: "hello" }]
    });

    expect(response.content).toBe("openai-response");
    expect(response.provider).toBe("openai");
  });

  it("throws LLMProviderNotFoundError if provider is missing", async () => {
    const gateway = new DefaultLLMGateway();

    await expect(
      gateway.complete({
        provider: "anthropic",
        model: "claude-test",
        messages: [{ role: "user", content: "hello" }]
      })
    ).rejects.toBeInstanceOf(LLMProviderNotFoundError);
  });

  it("throws LLMValidationError for invalid request", async () => {
    const gateway = new DefaultLLMGateway();
    gateway.registerAdapter(new MockLLMProviderAdapter({ provider: "mistral" }));

    await expect(
      gateway.complete({
        provider: "mistral",
        model: "",
        messages: []
      })
    ).rejects.toBeInstanceOf(LLMValidationError);
  });
});
