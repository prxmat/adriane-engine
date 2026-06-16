import type { AgentId } from "./types.js";

export type SwarmHandoff = {
  type: "swarm_handoff";
  goto: AgentId;
  update: {
    reason: string;
    [key: string]: unknown;
  };
};

export const createSwarmHandoff = (goto: AgentId, reason: string): SwarmHandoff => ({
  type: "swarm_handoff",
  goto,
  update: { reason }
});

export const isSwarmHandoff = (value: unknown): value is SwarmHandoff => {
  if (value === null || typeof value !== "object") {
    return false;
  }
  const record = value as Record<string, unknown>;
  return record.type === "swarm_handoff" && typeof record.goto === "string";
};
