import { describe, expect, it } from "vitest";

import { createGraph, DefaultLLMGateway } from "./index.js";

/**
 * `taskNode` (ADR 0022/0023, phase 1) is sugar over `subgraph`: the sub-agent runs as
 * a one-node child graph so the spawn inherits the runtime's checkpoint / audit /
 * human-gate guarantees with NO new runtime code. These tests assert the STRUCTURE it
 * compiles to (the subgraph node + isolating mappings + a terse child agent). The
 * end-to-end spawn / suspend-resume behaviour rides the subgraph path already covered
 * on the Rust engine by `subgraph.test.ts`.
 */

const llm = () => new DefaultLLMGateway();

const buildWithTask = (compress?: boolean) =>
  createGraph({ name: "research", id: "research" })
    .channel("objective", { type: "string", default: "" })
    .taskNode("dig", {
      subAgent: { llm: llm(), prompt: { system: "Research deeply." }, provider: "anthropic" },
      compress
    });

describe("GraphBuilder.taskNode", () => {
  it("compiles to a subgraph node that isolates the objective in and the report out", () => {
    const parts = buildWithTask().toSubgraphParts();
    const node = parts.definition.nodes.find((candidate) => String(candidate.id) === "dig");
    expect(node?.type).toBe("subgraph");
    // Only the objective crosses in; only the report crosses back.
    expect(node?.inputMapping).toEqual({ objective: "objective" });
    expect(node?.outputMapping).toEqual({ report: "report" });
    expect(String(node?.subgraphId)).toBe("dig-task");
  });

  it("registers the child graph and a TERSE sub-agent writing the report channel", () => {
    const parts = buildWithTask().toSubgraphParts();
    expect(parts.subgraphDefs.has("dig-task")).toBe(true);

    const childAgent = parts.agentConfigs.get("dig__agent");
    expect(childAgent).toBeDefined();
    expect(childAgent?.outputChannel).toBe("report");
    // compress defaults to true -> the sub-agent runs terse (a summary, not a transcript).
    expect(childAgent?.outputStyle).toBe("terse");
  });

  it("leaves the sub-agent un-terse when compress is false", () => {
    const parts = buildWithTask(false).toSubgraphParts();
    const childAgent = parts.agentConfigs.get("dig__agent");
    expect(childAgent?.outputStyle).toBeUndefined();
  });

  it("declares the report channel on the parent and compiles cleanly", () => {
    const compiled = buildWithTask().compile();
    const channels = compiled.definition.channels as Record<string, unknown>;
    expect(channels.report).toBeDefined();
    expect(channels.objective).toBeDefined();
    const node = compiled.definition.nodes.find((candidate) => String(candidate.id) === "dig");
    expect(node?.type).toBe("subgraph");
  });

  it("supports a custom objective/report channel pair", () => {
    const parts = createGraph({ name: "r2", id: "r2" })
      .channel("ask", { type: "string", default: "" })
      .taskNode("spawn", {
        subAgent: { llm: llm(), prompt: { system: "Go." }, provider: "anthropic" },
        objectiveChannel: "ask",
        reportChannel: "answer"
      })
      .toSubgraphParts();
    const node = parts.definition.nodes.find((candidate) => String(candidate.id) === "spawn");
    expect(node?.inputMapping).toEqual({ ask: "ask" });
    expect(node?.outputMapping).toEqual({ answer: "answer" });
    expect(parts.agentConfigs.get("spawn__agent")?.outputChannel).toBe("answer");
  });
});
