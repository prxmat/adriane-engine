import type { LLMProviderAdapter } from "./interfaces.js";
import type { LLMProvider, LLMResponse, LLMStreamChunk } from "./types.js";

type MockAdapterOptions = {
  provider: LLMProvider;
  response?: LLMResponse;
  /**
   * Scripted responses replayed one per `complete()` call (the last repeats once
   * exhausted). Lets a test drive a multi-turn agent — e.g. a `tool_use` turn
   * followed by a final-answer turn. Takes precedence over `response`.
   */
  responses?: LLMResponse[];
  chunks?: LLMStreamChunk[];
};

export class MockLLMProviderAdapter implements LLMProviderAdapter {
  public readonly provider: LLMProvider;
  private readonly responses: LLMResponse[];
  private readonly chunks: LLMStreamChunk[];
  private index = 0;

  public constructor(options: MockAdapterOptions) {
    this.provider = options.provider;
    const fallback: LLMResponse = {
      content: "mock-response",
      usage: { promptTokens: 1, completionTokens: 1 },
      model: "mock-model",
      provider: options.provider
    };
    this.responses =
      options.responses ?? (options.response !== undefined ? [options.response] : [fallback]);
    this.chunks = options.chunks ?? [{ delta: "mock-response", done: true }];
  }

  public async complete(): Promise<LLMResponse> {
    const next = this.responses[Math.min(this.index, this.responses.length - 1)];
    this.index += 1;
    // `responses` is non-empty by construction, so the fallback is unreachable.
    return next ?? this.responses[this.responses.length - 1]!;
  }

  public async *stream(): AsyncIterable<LLMStreamChunk> {
    for (const chunk of this.chunks) {
      yield chunk;
    }
  }
}
