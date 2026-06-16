import { describe, expect, it } from "vitest";

import { structuralEqual } from "./equality.js";

describe("structuralEqual", () => {
  it("compares primitives, including NaN", () => {
    expect(structuralEqual(1, 1)).toBe(true);
    expect(structuralEqual("a", "a")).toBe(true);
    expect(structuralEqual(1, 2)).toBe(false);
    expect(structuralEqual(NaN, NaN)).toBe(true);
    expect(structuralEqual(null, undefined)).toBe(false);
    expect(structuralEqual(0, false)).toBe(false);
  });

  it("is insensitive to object key order", () => {
    expect(structuralEqual({ a: 1, b: 2 }, { b: 2, a: 1 })).toBe(true);
  });

  it("detects differing values, missing keys, and extra keys", () => {
    expect(structuralEqual({ a: 1 }, { a: 2 })).toBe(false);
    expect(structuralEqual({ a: 1, b: 2 }, { a: 1 })).toBe(false);
    expect(structuralEqual({ a: 1 }, { a: 1, b: 2 })).toBe(false);
  });

  it("compares nested structures and arrays", () => {
    expect(structuralEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] })).toBe(true);
    expect(structuralEqual({ a: [1, 2] }, { a: [2, 1] })).toBe(false);
    expect(structuralEqual([1, 2, 3], [1, 2])).toBe(false);
  });

  it("compares Dates by instant", () => {
    expect(structuralEqual(new Date("2026-01-01"), new Date("2026-01-01"))).toBe(true);
    expect(structuralEqual(new Date("2026-01-01"), new Date("2026-01-02"))).toBe(false);
  });

  it("does not crash on circular references", () => {
    const a: Record<string, unknown> = { name: "x" };
    a.self = a;
    const b: Record<string, unknown> = { name: "x" };
    b.self = b;
    expect(structuralEqual(a, b)).toBe(true);

    const c: Record<string, unknown> = { name: "y" };
    c.self = c;
    expect(structuralEqual(a, c)).toBe(false); // differing primitive still caught
  });

  it("handles shared references without false positives", () => {
    const shared = { k: 1 };
    expect(structuralEqual({ a: shared, b: shared }, { a: { k: 1 }, b: { k: 2 } })).toBe(false);
  });
});
