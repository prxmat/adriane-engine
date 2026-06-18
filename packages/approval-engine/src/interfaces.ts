import type { NodeId, RunId } from "@adriane-ai/graph-core";
import type { ArtifactRef } from "@adriane-ai/artifact-store";

import type { ApprovalId, ApprovalRequest } from "./types.js";

export type RequestApprovalParams = {
  runId: RunId;
  nodeId: NodeId;
  requestedBy: string;
  subject: ArtifactRef | { description: string };
};

export interface ApprovalEngine {
  request(params: RequestApprovalParams): Promise<ApprovalRequest>;
  approve(id: ApprovalId, resolvedBy: string): Promise<ApprovalRequest>;
  reject(id: ApprovalId, resolvedBy: string, reason: string): Promise<ApprovalRequest>;
  getPending(runId?: RunId): Promise<ApprovalRequest[]>;
  getById(id: ApprovalId): Promise<ApprovalRequest | undefined>;
}
