import type { Brand } from "./types";

export type MessageId = Brand<string, "MessageId">;

export type ToolCall = {
  id: string;
  name: string;
  input: unknown;
};

export type BaseMessage = {
  id: MessageId;
  createdAt: Date;
  metadata?: Record<string, unknown>;
};

export type HumanMessage = BaseMessage & { role: "human"; content: string };
export type AIMessage = BaseMessage & { role: "ai"; content: string; toolCalls?: ToolCall[] };
export type ToolMessage = BaseMessage & { role: "tool"; toolCallId: string; content: string };
export type SystemMessage = BaseMessage & { role: "system"; content: string };

export type Message = HumanMessage | AIMessage | ToolMessage | SystemMessage;
