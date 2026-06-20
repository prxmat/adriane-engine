import type { LLMGateway, LLMProviderAdapter } from "./interfaces.js";
import type { LLMRequest, LLMResponse, LLMStreamChunk } from "./types.js";

/**
 * PII redaction seam (ADR 0008). The OSS engine ships only this interface and a no-op default;
 * the heavy detection (Presidio/GLiNER, or an OpenAI privacy pass) lives in a control-plane
 * implementation that the deployment injects. `redactRequest` anonymizes the prompt before it
 * reaches a provider; `hydrateResponse` restores the user's own values in the answer.
 */
export interface PiiRedactor {
  redactRequest(req: LLMRequest): Promise<LLMRequest> | LLMRequest;
  hydrateResponse(res: LLMResponse): Promise<LLMResponse> | LLMResponse;
}

/** Default: pass everything through unchanged (no redaction). */
export const noopPiiRedactor: PiiRedactor = {
  redactRequest: (req) => req,
  hydrateResponse: (res) => res
};

/**
 * Wraps any {@link LLMGateway} so every `complete()` call redacts the request before the
 * provider sees it and hydrates the response after. Compose it around `DefaultLLMGateway`:
 * `new RedactingLLMGateway(gateway, controlPlaneRedactor)`. Streaming passes through (chunk
 * re-hydration is provider-specific and left to a later refinement).
 */
export class RedactingLLMGateway implements LLMGateway {
  public constructor(
    private readonly inner: LLMGateway,
    private readonly redactor: PiiRedactor = noopPiiRedactor
  ) {}

  public registerAdapter(adapter: LLMProviderAdapter): void {
    this.inner.registerAdapter(adapter);
  }

  public async complete(req: LLMRequest): Promise<LLMResponse> {
    const redacted = await this.redactor.redactRequest(req);
    const response = await this.inner.complete(redacted);
    return this.redactor.hydrateResponse(response);
  }

  public async *stream(req: LLMRequest): AsyncIterable<LLMStreamChunk> {
    const redacted = await this.redactor.redactRequest(req);
    for await (const chunk of this.inner.stream(redacted)) {
      yield chunk;
    }
  }
}
