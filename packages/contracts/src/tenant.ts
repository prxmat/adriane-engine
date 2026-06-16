import { z } from "zod";

export const TenantQuotaDtoSchema = z.object({
  maxRunsPerDay: z.number().int().min(0),
  maxArtifactsStorageMb: z.number().int().min(0),
  maxConcurrentRuns: z.number().int().min(0)
});

export const TenantUsageDtoSchema = z.object({
  runsToday: z.number().int().min(0),
  artifactsStorageMb: z.number().min(0),
  concurrentRuns: z.number().int().min(0),
  updatedAt: z.string().datetime()
});

export const TenantDtoSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  plan: z.enum(["free", "pro", "enterprise"]),
  quota: TenantQuotaDtoSchema,
  usage: TenantUsageDtoSchema
});

export type TenantDto = z.infer<typeof TenantDtoSchema>;
export type TenantQuotaDto = z.infer<typeof TenantQuotaDtoSchema>;
export type TenantUsageDto = z.infer<typeof TenantUsageDtoSchema>;
