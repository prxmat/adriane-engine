import { z } from "zod";

import { ConfigValidationError } from "./errors.js";

const EnvironmentSchema = z.object({
  NODE_ENV: z.enum(["local", "staging", "production"]),
  PORT: z.coerce.number().int().positive().default(3000),
  DATABASE_URL: z.string().min(1),
  REDIS_URL: z.string().min(1),
  JWT_SECRET: z.string().min(1),
  JWT_EXPIRY: z.string().min(1).default("1h"),
  OPENAI_API_KEY: z.string().min(1).optional(),
  ANTHROPIC_API_KEY: z.string().min(1).optional(),
  MISTRAL_API_KEY: z.string().min(1).optional(),
  OTEL_ENDPOINT: z.string().min(1).optional(),
  LOG_LEVEL: z.enum(["debug", "info", "warn", "error"]).default("info")
});

export type AppEnv = z.infer<typeof EnvironmentSchema>;

let cachedEnv: AppEnv | null = null;

export const parseEnv = (source: NodeJS.ProcessEnv = process.env): AppEnv => {
  const parsed = EnvironmentSchema.safeParse(source);
  if (!parsed.success) {
    throw new ConfigValidationError(parsed.error.issues);
  }
  return parsed.data;
};

export const getEnv = (): AppEnv => {
  if (cachedEnv === null) {
    cachedEnv = parseEnv(process.env);
  }
  return cachedEnv;
};
