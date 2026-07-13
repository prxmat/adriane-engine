import { describe, expect, it } from "vitest";

import { aggregateRanks, anonymizeAndShuffle, parseRanking } from "./council.js";

describe("anonymizeAndShuffle (ADR 0013 / 0061)", () => {
  const answers = [
    { memberId: "gpt", content: "answer-gpt" },
    { memberId: "claude", content: "answer-claude" },
    { memberId: "mistral", content: "answer-mistral" }
  ];

  it("relabels A,B,C and keeps content + memberId for de-anonymization", () => {
    const field = anonymizeAndShuffle(answers, "seed1");
    expect(field.map((f) => f.label)).toEqual(["A", "B", "C"]);
    // every original answer survives (content + memberId retained), just reordered/relabeled.
    expect(new Set(field.map((f) => f.memberId))).toEqual(new Set(["gpt", "claude", "mistral"]));
    for (const f of field) {
      expect(f.content).toBe(answers.find((a) => a.memberId === f.memberId)!.content);
    }
  });

  it("is deterministic per seed (replay-faithful) and reorders across seeds", () => {
    expect(anonymizeAndShuffle(answers, "seed1")).toEqual(anonymizeAndShuffle(answers, "seed1"));
    const order = (seed: string) => anonymizeAndShuffle(answers, seed).map((f) => f.memberId);
    // The shuffle depends on the seed; at least one seed yields a non-input order.
    expect([order("s1"), order("s2"), order("s3")].some((o) => o.join() !== "gpt,claude,mistral")).toBe(
      true
    );
  });
});

describe("aggregateRanks (Borda)", () => {
  it("ranks the field by summed Borda score, best-first", () => {
    // Two reviewers both prefer B over A over C → B wins, then A, then C.
    const out = aggregateRanks(
      [
        ["B", "A", "C"],
        ["B", "A", "C"]
      ],
      ["A", "B", "C"]
    );
    expect(out).toEqual(["B", "A", "C"]);
  });

  it("breaks ties by label asc and scores unranked labels 0", () => {
    // One reviewer ranks only A; B and C stay at 0 → A first, then B,C by label.
    expect(aggregateRanks([["A"]], ["A", "B", "C"])).toEqual(["A", "B", "C"]);
  });

  it("ignores unknown/duplicate labels in a ranking", () => {
    expect(aggregateRanks([["B", "B", "Z", "A"]], ["A", "B"])).toEqual(["B", "A"]);
  });
});

describe("parseRanking", () => {
  it("extracts named labels in order from prose, deduped", () => {
    expect(parseRanking("I rank B first, then A, then C.", ["A", "B", "C"])).toEqual(["B", "A", "C"]);
  });

  it("drops unknown labels and is empty when none match", () => {
    expect(parseRanking("none of these", ["A", "B"])).toEqual([]);
  });
});

import { council } from "./council.js";
import { DefaultLLMGateway, rustEngineAvailable, validateGraph } from "./index.js";

const describeIfRust = rustEngineAvailable() ? describe : describe.skip;

describeIfRust("council(...) graph (ADR 0061 E2b, Rust engine)", () => {
  const seat = () => ({ llm: new DefaultLLMGateway(), prompt: { system: "s" }, provider: "mistral" as const });

  it("wires dispatch → members (fan-out) → anonymize → reviewers (fan-out) → aggregate → chair", () => {
    const def = council({ members: [seat(), seat(), seat()], chair: seat() }).definition;
    expect(validateGraph(def)).toEqual([]);
    const ids = def.nodes.map((n) => String(n.id));
    expect(ids).toEqual(
      expect.arrayContaining([
        "dispatch",
        "member_0",
        "member_1",
        "member_2",
        "anonymize",
        "review_0",
        "review_1",
        "review_2",
        "aggregate",
        "chair"
      ])
    );
    // dispatch fans out to all members, joining at anonymize; anonymize fans out to reviewers.
    const dispatch = def.nodes.find((n) => String(n.id) === "dispatch");
    expect((dispatch as { fanOut?: { parallelTo: string[]; joinAt: string } }).fanOut).toEqual({
      parallelTo: ["member_0", "member_1", "member_2"],
      joinAt: "anonymize"
    });
    const anonymize = def.nodes.find((n) => String(n.id) === "anonymize");
    expect(
      (anonymize as { fanOut?: { joinAt: string } }).fanOut?.joinAt
    ).toBe("aggregate");
  });

  it("inserts an optional human gate before the chair", () => {
    const def = council({ members: [seat(), seat()], chair: seat(), humanGate: true }).definition;
    expect(def.nodes.map((n) => String(n.id))).toContain("gate");
    expect(validateGraph(def)).toEqual([]);
  });
});
