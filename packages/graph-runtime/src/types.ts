import type { ChannelsSchema, GraphState, NodeId, RunId } from "@adriane-ai/graph-core";

export type CheckpointId = string & { readonly __brand: "CheckpointId" };

export type Checkpoint<TChannels extends ChannelsSchema = ChannelsSchema> = {
  id: CheckpointId;
  runId: RunId;
  graphState: GraphState<TChannels>;
  createdAt: string;
};

export type RunEvent =
  | { type: "node_started"; runId: RunId; nodeId: NodeId; timestamp: string }
  | {
      type: "node_completed";
      runId: RunId;
      nodeId: NodeId;
      output: unknown;
      timestamp: string;
    }
  | {
      type: "node_failed";
      runId: RunId;
      nodeId: NodeId;
      error: string;
      attempt: number;
      timestamp: string;
    }
  | {
      type: "run_suspended";
      runId: RunId;
      nodeId: NodeId;
      reason: string;
      timestamp: string;
    }
  | { type: "run_resumed"; runId: RunId; nodeId: NodeId; timestamp: string }
  | {
      type: "run_completed";
      runId: RunId;
      finalState: GraphState<ChannelsSchema>;
      timestamp: string;
    }
  | { type: "run_failed"; runId: RunId; error: string; timestamp: string };
