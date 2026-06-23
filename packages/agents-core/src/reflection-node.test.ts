import { describe, expect, it } from "vitest";
import { DefaultLLMGateway, MockLLMProviderAdapter } from "../../llm-gateway/src/index.js";

import {
  createReflectionNode,
  critiqueRequestsRevision,
  parseReflectionCritique,
  REFLECTION_ISSUES_KEY
} from "./reflection-node.js";

const nodeWith = (content: string, opts?: { maxReflections?: number; scoreThreshold?: number }) => {
  const gateway = new DefaultLLMGateway();
  gateway.registerAdapter(
    new MockLLMProviderAdapter({
      provider: "openai",
      response: { content, usage: { promptTokens: 1, completionTokens: 1 }, model: "mock", provider: "openai" }
    })
  );
  return createReflectionNode({
    llm: gateway,
    previousNodeId: "prev" as never,
    maxReflections: opts?.maxReflections ?? 2,
    scoreThreshold: opts?.scoreThreshold
  });
};

describe("ReflectionNode", () => {
  it("falls back to the substring heuristic for an unstructured critique (legacy behaviour)", async () => {
    const node = nodeWith("problem detected, retry");
    const result = await node({}, {} as never, { memory: {} as never });
    expect((result as { goto?: unknown }).goto).toBe("prev");
  });

  it("accepts when the structured critique scores at/above the threshold", async () => {
    const node = nodeWith('{"ok": false, "score": 0.9, "issues": []}', { scoreThreshold: 0.8 });
    const result = await node({}, {} as never, { memory: {} as never });
    expect((result as { goto?: unknown }).goto).toBeUndefined();
    expect((result as { confidence?: number }).confidence).toBeCloseTo(0.6, 5);
  });

  it("revises and surfaces issues when the structured score is below the threshold", async () => {
    const node = nodeWith('Here is my critique: {"ok": false, "score": 0.3, "issues": ["weak intro", "no sources"]}', {
      scoreThreshold: 0.8
    });
    const result = (await node({}, {} as never, { memory: {} as never })) as {
      goto?: unknown;
      update?: Record<string, unknown>;
    };
    expect(result.goto).toBe("prev");
    expect(result.update?.[REFLECTION_ISSUES_KEY]).toEqual(["weak intro", "no sources"]);
  });

  it("accepts when ok:true even with a low score", async () => {
    const node = nodeWith('{"ok": true, "score": 0.1, "issues": []}');
    const result = await node({}, {} as never, { memory: {} as never });
    expect((result as { goto?: unknown }).goto).toBeUndefined();
  });

  it("stops revising once the reflection budget is exhausted", async () => {
    const node = nodeWith('{"ok": false, "score": 0.0, "issues": ["x"]}', { maxReflections: 0 });
    const result = await node({}, {} as never, { memory: {} as never });
    expect((result as { goto?: unknown }).goto).toBeUndefined(); // budget 0 → accept despite low score
  });
});

describe("parseReflectionCritique", () => {
  it("parses plain JSON", () => {
    expect(parseReflectionCritique('{"ok":true,"score":0.95,"issues":[]}')).toEqual({ ok: true, score: 0.95, issues: [] });
  });
  it("extracts JSON wrapped in prose / markdown", () => {
    expect(parseReflectionCritique('Sure!\n```json\n{"ok":false,"score":0.4,"issues":["a"]}\n```')).toEqual({
      ok: false,
      score: 0.4,
      issues: ["a"]
    });
  });
  it("clamps out-of-range scores", () => {
    expect(parseReflectionCritique('{"score": 1.7}')?.score).toBe(1);
    expect(parseReflectionCritique('{"score": -2}')?.score).toBe(0);
  });
  it("returns null for non-critique text", () => {
    expect(parseReflectionCritique("looks good to me")).toBeNull();
    expect(parseReflectionCritique('{"unrelated": 1}')).toBeNull();
  });
});

describe("critiqueRequestsRevision", () => {
  it("structured: revises below threshold, accepts at/above", () => {
    expect(critiqueRequestsRevision('{"ok":false,"score":0.2,"issues":["x"]}', 0.8)).toEqual({
      revise: true,
      issues: ["x"]
    });
    expect(critiqueRequestsRevision('{"ok":false,"score":0.85,"issues":[]}', 0.8).revise).toBe(false);
  });
  it("unstructured: legacy substring fallback", () => {
    expect(critiqueRequestsRevision("there is a problem").revise).toBe(true);
    expect(critiqueRequestsRevision("looks great").revise).toBe(false);
  });
});
