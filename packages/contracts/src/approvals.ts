import { z } from "zod";

const ApprovalStatusSchema = z.enum(["pending", "approved", "rejected"]);

export const ApprovalDtoSchema = z.object({
  id: z.string().min(1),
  runId: z.string().min(1),
  nodeId: z.string().min(1),
  requestedBy: z.string().min(1),
  /** Human-readable subject (e.g. `tool:refund`). */
  subject: z.string().min(1),
  status: ApprovalStatusSchema,
  resolvedBy: z.string().min(1).optional(),
  reason: z.string().min(1).optional(),
  requestedAt: z.string().datetime(),
  decidedAt: z.string().datetime().optional()
});

export type ApprovalDto = z.infer<typeof ApprovalDtoSchema>;

export const ApproveApprovalDtoSchema = z.object({
  resolvedBy: z.string().min(1)
});

export const RejectApprovalDtoSchema = z.object({
  resolvedBy: z.string().min(1),
  reason: z.string().min(1)
});

export type ApproveApprovalDto = z.infer<typeof ApproveApprovalDtoSchema>;
export type RejectApprovalDto = z.infer<typeof RejectApprovalDtoSchema>;
