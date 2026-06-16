import type { NodeId } from "@adriane/graph-core";

export type FanOutPlan = {
  parallelTo: NodeId[];
  joinAt: NodeId;
};
