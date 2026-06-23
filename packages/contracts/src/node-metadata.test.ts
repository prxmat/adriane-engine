import { describe, expect, it } from "vitest";

import { AgentNodeMetadataSchema, readAgentMetadata } from "./node-metadata.js";

/**
 * The agent carrier is the API↔Studio boundary for a persisted agent node. The
 * ADR 0014 (outputStyle/contextBudget) and ADR 0022/0023 (todosChannel) knobs must
 * SURVIVE a parse round-trip — Zod strips unknown keys, so a field missing from the
 * schema would be silently dropped on the catalog/Studio path (the bug this guards).
 */
describe("@adriane-ai/contracts — AgentNodeMetadataSchema", () => {
  it("keeps outputStyle, contextBudget and todosChannel through a parse round-trip", () => {
    const carrier = {
      provider: "anthropic",
      toolNames: ["writeTodos"],
      outputChannel: "agentResult",
      outputStyle: "terse" as const,
      contextBudget: 4000,
      todosChannel: "__todos"
    };
    const parsed = AgentNodeMetadataSchema.parse(carrier);
    expect(parsed.outputStyle).toBe("terse");
    expect(parsed.contextBudget).toBe(4000);
    expect(parsed.todosChannel).toBe("__todos");
  });

  it("reads the carrier off an open metadata bag without dropping the new fields", () => {
    const meta = readAgentMetadata({ agent: { provider: "anthropic", todosChannel: "__todos" } });
    expect(meta?.todosChannel).toBe("__todos");
  });

  it("rejects an outputStyle other than terse", () => {
    expect(AgentNodeMetadataSchema.safeParse({ outputStyle: "verbose" }).success).toBe(false);
  });

  it("leaves the new fields undefined when omitted", () => {
    const parsed = AgentNodeMetadataSchema.parse({ provider: "anthropic" });
    expect(parsed.todosChannel).toBeUndefined();
    expect(parsed.outputStyle).toBeUndefined();
    expect(parsed.contextBudget).toBeUndefined();
  });
});
