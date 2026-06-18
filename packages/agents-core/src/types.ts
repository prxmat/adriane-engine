import type { GraphState, NodeId } from "@adriane-ai/graph-core";
import type { ArtifactRef } from "@adriane-ai/artifact-store";
import type { BaseStore } from "../../memory-store/src/interfaces.js";
import type { CallbackManager } from "../../callbacks/src/interfaces.js";
import type { WorkingMemory } from "./working-memory.js";

export type AgentId = string & { readonly __brand: "AgentId" };

export type Blocker = {
  code: string;
  message: string;
  nodeId?: NodeId;
};

export type AgentResult = {
  artifacts: ArtifactRef[];
  blockers: Blocker[];
  approvalRequests: Array<{ subject: ArtifactRef | { description: string }; reason: string }>;
  confidence: number;
  reasoning: string;
  requiresHumanReview: boolean;
};

export type AgentRunFn<TInput = unknown> = (
  input: TInput,
  state: GraphState,
  context: { memory: BaseStore; workingMemory: WorkingMemory; callbacks?: CallbackManager }
) => Promise<AgentResult>;

export type ConsolidatedAgentResult = AgentResult & {
  channelUpdates: Record<string, unknown>;
  conflicts: string[];
};
