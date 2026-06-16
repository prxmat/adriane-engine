import type { NodeId, RunId } from "@adriane/graph-core";

export type SendEnvelope = {
  runId: RunId;
  nodeId: NodeId;
  input: unknown;
};
