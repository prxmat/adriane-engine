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
  | { type: "run_failed"; runId: RunId; error: string; timestamp: string }
  // ADR 0033 phase 13: one observational per-token delta during agent generation.
  // Observational-only — never persisted (it bypasses the EventBus on the Rust path),
  // so it is absent from checkpoints and the journal. `messageId` groups all deltas of
  // one agent turn; `spawnId`/`parentRunId` tag a `mapAgents` sub-agent's stream so a
  // consumer can demultiplex concurrent spawns (both absent for a top-level agent node).
  | {
      type: "token_delta";
      runId: RunId;
      nodeId: NodeId;
      messageId: string;
      delta: string;
      parentRunId?: RunId;
      spawnId?: number;
      timestamp: string;
    };
