import type { NodeId } from "@adriane-ai/graph-core";

export type FanOutPlan = {
  parallelTo: NodeId[];
  joinAt: NodeId;
};
