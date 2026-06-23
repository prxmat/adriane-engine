export const LLM_PROVIDERS = ["openai", "anthropic", "mistral", "ollama", "mock"] as const;
export type LLMProvider = (typeof LLM_PROVIDERS)[number];

export type LLMModel = string;

/** A text span in a structured message. */
export type LLMTextBlock = { type: "text"; text: string };

/** An assistant turn requesting a tool call. */
export type LLMToolUseBlock = { type: "tool_use"; id: string; name: string; input: unknown };

/** A user turn returning a tool's result, paired to a prior `tool_use` by id. */
export type LLMToolResultBlock = {
  type: "tool_result";
  toolUseId: string;
  content: string;
  isError?: boolean;
};

export type LLMContentBlock = LLMTextBlock | LLMToolUseBlock | LLMToolResultBlock;

// ADR 0030 (phase 9): multimodal media blocks (image / audio / file) + `LLMMediaSource`
// live on the Rust `ContentBlock`/`MediaSource` (the engine execution path). The TS
// parity types + the deprecated TS adapters' fan-out land with the SDK authoring entry
// point (ADR 0030 9e) — deferred so this foundation PR does not touch the dead TS engine.

export type LLMMessage = {
  role: "system" | "user" | "assistant";
  /**
   * Either plain text or a list of content blocks. Blocks carry `tool_use` /
   * `tool_result` turns so a tool-calling agent can hold a real multi-turn
   * conversation with the provider instead of stuffing observations into text.
   */
  content: string | LLMContentBlock[];
};

/**
 * Tool definition exposed to the provider. Part of the cacheable prefix: the tool
 * list must be deterministic (stable order, stable schema) or it busts the cache.
 */
export type LLMToolDef = {
  name: string;
  description?: string;
  inputSchema: Record<string, unknown>;
};

export type LLMRequest = {
  provider: LLMProvider;
  model: LLMModel;
  messages: LLMMessage[];
  /**
   * Immutable system prompt. Forms the cacheable prefix together with `tools`.
   * Keep volatile content (dates, timestamps, session ids) OUT of here.
   */
  system?: string;
  tools?: LLMToolDef[];
  maxTokens?: number;
  temperature?: number;
  stream?: boolean;
};

/** A structured tool call surfaced by the provider (e.g. an Anthropic `tool_use` block). */
export type LLMToolCall = {
  id: string;
  name: string;
  input: unknown;
};

export type LLMResponse = {
  content: string;
  /**
   * Structured tool calls the model wants executed. Present when the provider
   * stops on a tool-use turn; consumers should run these instead of parsing the
   * text content for a tool protocol.
   */
  toolCalls?: LLMToolCall[];
  /** Why the model stopped — `"tool_use"` signals it is waiting on tool results. */
  stopReason?: "end_turn" | "tool_use" | "max_tokens" | "stop_sequence" | string;
  usage: {
    promptTokens: number;
    completionTokens: number;
    /** Tokens served from the prompt cache (~0.1x cost). */
    cacheReadTokens?: number;
    /** Tokens written to the prompt cache this call (~1.25x cost). */
    cacheWriteTokens?: number;
  };
  model: string;
  provider: LLMProvider;
};

export type LLMStreamChunk = {
  delta: string;
  done: boolean;
};
