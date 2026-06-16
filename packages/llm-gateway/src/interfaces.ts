import type { LLMProvider, LLMRequest, LLMResponse, LLMStreamChunk } from "./types.js";

export interface LLMProviderAdapter {
  provider: LLMProvider;
  complete(req: LLMRequest): Promise<LLMResponse>;
  stream(req: LLMRequest): AsyncIterable<LLMStreamChunk>;
}

export interface LLMGateway {
  complete(req: LLMRequest): Promise<LLMResponse>;
  stream(req: LLMRequest): AsyncIterable<LLMStreamChunk>;
  registerAdapter(adapter: LLMProviderAdapter): void;
}
