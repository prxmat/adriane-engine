import type { CallbackEvent } from "./types.js";

export interface CallbackHandler {
  onLLMStart?(event: Extract<CallbackEvent, { type: "onLLMStart" }>): void | Promise<void>;
  onLLMToken?(event: Extract<CallbackEvent, { type: "onLLMToken" }>): void | Promise<void>;
  onLLMEnd?(event: Extract<CallbackEvent, { type: "onLLMEnd" }>): void | Promise<void>;
  onLLMError?(event: Extract<CallbackEvent, { type: "onLLMError" }>): void | Promise<void>;
  onToolStart?(event: Extract<CallbackEvent, { type: "onToolStart" }>): void | Promise<void>;
  onToolEnd?(event: Extract<CallbackEvent, { type: "onToolEnd" }>): void | Promise<void>;
  onToolError?(event: Extract<CallbackEvent, { type: "onToolError" }>): void | Promise<void>;
  onNodeStart?(event: Extract<CallbackEvent, { type: "onNodeStart" }>): void | Promise<void>;
  onNodeEnd?(event: Extract<CallbackEvent, { type: "onNodeEnd" }>): void | Promise<void>;
  onNodeError?(event: Extract<CallbackEvent, { type: "onNodeError" }>): void | Promise<void>;
  onChainStart?(event: Extract<CallbackEvent, { type: "onChainStart" }>): void | Promise<void>;
  onChainEnd?(event: Extract<CallbackEvent, { type: "onChainEnd" }>): void | Promise<void>;
  onChainError?(event: Extract<CallbackEvent, { type: "onChainError" }>): void | Promise<void>;
  onAgentAction?(event: Extract<CallbackEvent, { type: "onAgentAction" }>): void | Promise<void>;
  onAgentFinish?(event: Extract<CallbackEvent, { type: "onAgentFinish" }>): void | Promise<void>;
}

export interface CallbackManager {
  addHandler(handler: CallbackHandler): void;
  removeHandler(handler: CallbackHandler): void;
  emit(event: CallbackEvent): Promise<void>;
  createChild(tags?: string[], metadata?: Record<string, unknown>): CallbackManager;
}
