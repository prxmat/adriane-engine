import { describe, expect, it } from "vitest";
import { verifyReplayDecisions, type ReplayDecision } from "./verify-replay.js";

const dec = (status: string, subject: string): ReplayDecision => ({ status, subject });

describe("verifyReplayDecisions", () => {
  it("passes when the replay reproduces every decision in order", () => {
    const attested = [dec("approved", "deploy prod"), dec("rejected", "wire $5000")];
    const replayed = [dec("approved", "deploy prod"), dec("rejected", "wire $5000")];
    const result = verifyReplayDecisions(attested, replayed);
    expect(result.ok).toBe(true);
    expect(result.mismatches).toHaveLength(0);
  });

  it("flags a flipped status as a mismatch at its index", () => {
    const attested = [dec("approved", "deploy prod")];
    const replayed = [dec("rejected", "deploy prod")];
    const result = verifyReplayDecisions(attested, replayed);
    expect(result.ok).toBe(false);
    expect(result.mismatches).toEqual([
      { index: 0, attested: dec("approved", "deploy prod"), replayed: dec("rejected", "deploy prod") }
    ]);
  });

  it("flags a diverging subject even when the status matches", () => {
    const result = verifyReplayDecisions([dec("approved", "wire $100")], [dec("approved", "wire $999")]);
    expect(result.ok).toBe(false);
    expect(result.mismatches[0]?.index).toBe(0);
  });

  it("treats reordered decisions as mismatches (order is significant)", () => {
    const a = [dec("approved", "A"), dec("rejected", "B")];
    const b = [dec("rejected", "B"), dec("approved", "A")];
    const result = verifyReplayDecisions(a, b);
    expect(result.ok).toBe(false);
    expect(result.mismatches).toHaveLength(2);
  });

  it("reports a decision the replay dropped (shorter than attested)", () => {
    const result = verifyReplayDecisions([dec("approved", "A"), dec("approved", "B")], [dec("approved", "A")]);
    expect(result.ok).toBe(false);
    expect(result.mismatches).toEqual([{ index: 1, attested: dec("approved", "B"), replayed: undefined }]);
  });

  it("reports a decision the replay invented (longer than attested)", () => {
    const result = verifyReplayDecisions([dec("approved", "A")], [dec("approved", "A"), dec("approved", "X")]);
    expect(result.ok).toBe(false);
    expect(result.mismatches).toEqual([{ index: 1, attested: undefined, replayed: dec("approved", "X") }]);
  });

  it("an empty chain against an empty replay is vacuously ok", () => {
    expect(verifyReplayDecisions([], []).ok).toBe(true);
  });
});
