import type { RunId } from "@adriane-ai/graph-core";

export const createForkRunId = (runId: RunId): RunId =>
  `${String(runId)}:fork:${Date.now()}:${Math.random().toString(36).slice(2, 8)}` as RunId;
