import type { ApprovalId } from "./types.js";

export class ApprovalSelfApprovalError extends Error {
  public constructor(approvalId: ApprovalId) {
    super(`Self-approval is forbidden for approval '${String(approvalId)}'.`);
    this.name = "ApprovalSelfApprovalError";
  }
}

export class ApprovalAlreadyResolvedError extends Error {
  public constructor(approvalId: ApprovalId) {
    super(`Approval '${String(approvalId)}' has already been resolved.`);
    this.name = "ApprovalAlreadyResolvedError";
  }
}

export class ApprovalNotFoundError extends Error {
  public constructor(approvalId: ApprovalId) {
    super(`Approval '${String(approvalId)}' was not found.`);
    this.name = "ApprovalNotFoundError";
  }
}
