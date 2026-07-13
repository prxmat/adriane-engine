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
