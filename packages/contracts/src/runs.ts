import { z } from "zod";

const GraphStatusSchema = z.enum(["idle", "running", "suspended", "completed", "failed"]);

export const CreateRunDtoSchema = z.object({
  graphId: z.string().min(1),
  initialData: z.record(z.string(), z.unknown()).default({})
});

export const RunDtoSchema = z.object({
  id: z.string().min(1),
  graphId: z.string().min(1),
  status: GraphStatusSchema,
  currentNodeId: z.string().min(1).optional(),
  channels: z.record(z.string(), z.unknown()),
  version: z.number().int().min(0),
  checkpointId: z.string().min(1).optional(),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime()
});

/** Lightweight row for the runs list — no channels, cheap to query at scale. */
export const RunSummaryDtoSchema = z.object({
  id: z.string().min(1),
  graphId: z.string().min(1),
  status: GraphStatusSchema,
  currentNodeId: z.string().min(1).nullable(),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime()
});

export const RunListDtoSchema = z.object({
  items: z.array(RunSummaryDtoSchema),
  total: z.number().int().min(0),
  limit: z.number().int().min(1),
  offset: z.number().int().min(0)
});

/** Query params arrive as strings over HTTP — coerce numerics. */
export const ListRunsQueryDtoSchema = z.object({
  status: GraphStatusSchema.optional(),
  graphId: z.string().min(1).optional(),
  limit: z.coerce.number().int().min(1).max(200).default(50),
  offset: z.coerce.number().int().min(0).default(0)
});

export const InterruptRunDtoSchema = z.object({
  nodeId: z.string().min(1),
  when: z.enum(["before", "after"])
});

export const PatchRunStateDtoSchema = z.object({
  patch: z.record(z.string(), z.unknown()),
  resumeFrom: z.string().min(1).optional()
});

export const ReplayRunDtoSchema = z.object({
  checkpointId: z.string().min(1)
});

export const CheckpointDtoSchema = z.object({
  id: z.string().min(1),
  runId: z.string().min(1),
  graphState: z.object({
    runId: z.string().min(1),
    graphId: z.string().min(1),
    currentNodeId: z.string().min(1),
    status: GraphStatusSchema,
    channels: z.record(z.string(), z.unknown()),
    version: z.number().int().min(0),
    checkpointId: z.string().min(1).optional(),
    createdAt: z.string().datetime(),
    updatedAt: z.string().datetime()
  }),
  createdAt: z.string().datetime()
});

export type CreateRunDto = z.infer<typeof CreateRunDtoSchema>;
export type RunDto = z.infer<typeof RunDtoSchema>;
export type RunSummaryDto = z.infer<typeof RunSummaryDtoSchema>;
export type RunListDto = z.infer<typeof RunListDtoSchema>;
export type ListRunsQueryDto = z.infer<typeof ListRunsQueryDtoSchema>;
export type InterruptRunDto = z.infer<typeof InterruptRunDtoSchema>;
export type PatchRunStateDto = z.infer<typeof PatchRunStateDtoSchema>;
export type ReplayRunDto = z.infer<typeof ReplayRunDtoSchema>;
export type CheckpointDto = z.infer<typeof CheckpointDtoSchema>;
