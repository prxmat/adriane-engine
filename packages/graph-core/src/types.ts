import type { Message } from "./messages";

export type Brand<T, TBrand extends string> = T & { readonly __brand: TBrand };

export type NodeId = Brand<string, "NodeId">;
export type EdgeId = Brand<string, "EdgeId">;
export type GraphId = Brand<string, "GraphId">;
export type RunId = Brand<string, "RunId">;
export type ChannelReducer = "replace" | "append" | "merge";

export type ChannelDefinition<T> = {
  type: string;
  reducer: ChannelReducer;
  default?: T;
  /** ADR 0032: never emit this channel's value in run events/logs (masked; still checkpointed). */
  noLog?: boolean;
};

export type MessagesChannelDefinition = {
  type: "messages";
  reducer: "append";
  default?: Message[];
};

export type ChannelsSchema = Record<string, ChannelDefinition<unknown>>;

export type ResolvedChannels<TChannels extends ChannelsSchema> = {
  [K in keyof TChannels]: TChannels[K] extends ChannelDefinition<infer TValue> ? TValue : never;
};

export const NODE_TYPES = ["action", "agent", "tool", "human-gate", "subgraph"] as const;
export type NodeType = (typeof NODE_TYPES)[number];

export const EDGE_TYPES = ["default", "conditional"] as const;
export type EdgeType = (typeof EDGE_TYPES)[number];

export const GRAPH_STATUSES = ["idle", "running", "suspended", "completed", "failed"] as const;
export type GraphStatus = (typeof GRAPH_STATUSES)[number];

export type RetryPolicy = {
  maxAttempts: number;
  backoffMs: number;
};

export type Command<TChannels extends ChannelsSchema = ChannelsSchema> = {
  goto: NodeId | NodeId[];
  update?: Partial<ResolvedChannels<TChannels>>;
};

export type NodeDefinition = {
  id: NodeId;
  type: NodeType;
  label: string;
  subgraphId?: GraphId;
  inputMapping?: Record<string, string>;
  outputMapping?: Record<string, string>;
  fanOut?: { parallelTo: NodeId[]; joinAt: NodeId };
  retryPolicy?: RetryPolicy;
  metadata?: Record<string, unknown>;
};

export type EdgeDefinition = {
  id: EdgeId;
  from: NodeId;
  to: NodeId;
  type: EdgeType;
  condition?: string;
};

export type GraphState<TChannels extends ChannelsSchema = ChannelsSchema> = {
  runId: RunId;
  graphId: GraphId;
  currentNodeId: NodeId;
  status: GraphStatus;
  channels: ResolvedChannels<TChannels>;
  version: number;
  checkpointId?: string;
  createdAt: string;
  updatedAt: string;
};

export type GraphDefinition<TChannels extends ChannelsSchema = ChannelsSchema> = {
  id: GraphId;
  version: string;
  name: string;
  recursionLimit?: number;
  channels: TChannels;
  nodes: NodeDefinition[];
  edges: EdgeDefinition[];
  entryNodeId: NodeId;
  metadata?: Record<string, unknown>;
};

export const trimMessages = (
  messages: Message[],
  maxTokens: number,
  countFn: (message: Message) => number
): Message[] => {
  if (maxTokens <= 0) {
    return [];
  }
  const reversed: Message[] = [];
  let total = 0;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message === undefined) {
      continue;
    }
    const cost = countFn(message);
    if (total + cost > maxTokens) {
      continue;
    }
    total += cost;
    reversed.push(message);
  }
  return reversed.reverse();
};

export const filterMessages = (messages: Message[], roles: Message["role"][]): Message[] => {
  const allowed = new Set(roles);
  return messages.filter((message) => allowed.has(message.role));
};
