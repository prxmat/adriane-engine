import type { NodeId, RunId } from "@adriane-ai/graph-core";

export type SendEnvelope = {
  runId: RunId;
  nodeId: NodeId;
  input: unknown;
};
