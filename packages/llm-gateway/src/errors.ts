import type { LLMProvider } from "./types.js";

export class LLMProviderNotFoundError extends Error {
  public readonly provider: LLMProvider;

  public constructor(provider: LLMProvider) {
    super(`No LLM adapter registered for provider '${provider}'.`);
    this.name = "LLMProviderNotFoundError";
    this.provider = provider;
  }
}

export class LLMValidationError extends Error {
  public readonly issues: string[];

  public constructor(issues: string[]) {
    super("Invalid LLM request.");
    this.name = "LLMValidationError";
    this.issues = issues;
  }
}

/**
 * A provider returned a non-2xx HTTP response. Carries the status and the raw response
 * body so callers can surface the upstream error verbatim instead of a bare throw.
 */
export class LLMProviderError extends Error {
  public readonly status: number;
  public readonly body: string;

  public constructor(status: number, body: string) {
    super(`LLM provider request failed with status ${status}.`);
    this.name = "LLMProviderError";
    this.status = status;
    this.body = body;
  }
}
