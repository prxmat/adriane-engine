type BaseCallbackEvent = {
  runId: string;
  nodeId?: string;
  tags?: string[];
  metadata?: Record<string, unknown>;
  timestamp: string;
};

export type CallbackEvent =
  | (BaseCallbackEvent & { type: "onLLMStart"; input: unknown })
  | (BaseCallbackEvent & { type: "onLLMToken"; token: string })
  | (BaseCallbackEvent & { type: "onLLMEnd"; output: unknown })
  | (BaseCallbackEvent & { type: "onLLMError"; error: string })
  | (BaseCallbackEvent & { type: "onToolStart"; tool: string; input: unknown })
  | (BaseCallbackEvent & { type: "onToolEnd"; tool: string; output: unknown })
  | (BaseCallbackEvent & { type: "onToolError"; tool: string; error: string })
  | (BaseCallbackEvent & { type: "onNodeStart"; input: unknown })
  | (BaseCallbackEvent & { type: "onNodeEnd"; output: unknown })
  | (BaseCallbackEvent & { type: "onNodeError"; error: string })
  | (BaseCallbackEvent & { type: "onChainStart"; input: unknown })
  | (BaseCallbackEvent & { type: "onChainEnd"; output: unknown })
  | (BaseCallbackEvent & { type: "onChainError"; error: string })
  | (BaseCallbackEvent & { type: "onAgentAction"; action: string; payload?: unknown })
  | (BaseCallbackEvent & { type: "onAgentFinish"; result: unknown });
