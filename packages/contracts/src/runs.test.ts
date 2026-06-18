import { describe, expect, it } from "vitest";

import { PatchRunStateDtoSchema, RESERVED_PATCH_CHANNELS } from "./runs.js";

/**
 * The patch-run-state contract gates the `PATCH /runs/:id/state` boundary. A client
 * may write ordinary channels, but never the reserved governance channels
 * (`__approvedTools` / `__approvalIds`) — writing those directly would be a
 * self-approval back door (forge an unlock, or erase the pending ids the resume gate
 * reads). The schema rejects such a patch so it surfaces as a 400/422, not a silent
 * state mutation.
 */
describe("@adriane-ai/contracts — PatchRunStateDtoSchema reserved-channel gate", () => {
  it("accepts a patch of ordinary channels", () => {
    const result = PatchRunStateDtoSchema.safeParse({
      patch: { draft: "hello", count: 3 },
      resumeFrom: "node-a"
    });
    expect(result.success).toBe(true);
  });

  it("rejects a patch that writes __approvedTools", () => {
    const result = PatchRunStateDtoSchema.safeParse({
      patch: { __approvedTools: ["refund"] }
    });
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0]?.message).toContain("__approvedTools");
    }
  });

  it("rejects a patch that writes __approvalIds", () => {
    const result = PatchRunStateDtoSchema.safeParse({
      patch: { __approvalIds: [] }
    });
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0]?.message).toContain("__approvalIds");
    }
  });

  it("rejects a patch that mixes a reserved channel with ordinary ones", () => {
    const result = PatchRunStateDtoSchema.safeParse({
      patch: { draft: "ok", __approvedTools: ["wire"] }
    });
    expect(result.success).toBe(false);
  });

  it("names both reserved channels", () => {
    expect([...RESERVED_PATCH_CHANNELS]).toEqual(["__approvedTools", "__approvalIds"]);
  });
});
