import { describe, expect, it } from "vitest";

import { getAllFlags, isEnabled } from "./feature-flags.js";

describe("feature flags", () => {
  it("returns true when flag is enabled", () => {
    const enabled = isEnabled("streaming", {
      FEATURE_STREAMING: "true"
    });
    expect(enabled).toBe(true);
  });

  it("returns false when flag is disabled or missing", () => {
    const disabledExplicit = isEnabled("streaming", {
      FEATURE_STREAMING: "false"
    });
    const disabledMissing = isEnabled("subgraphs", {});
    expect(disabledExplicit).toBe(false);
    expect(disabledMissing).toBe(false);
  });

  it("returns all flags as a record", () => {
    const flags = getAllFlags({
      FEATURE_STREAMING: "true",
      FEATURE_SUBGRAPHS: "false",
      FEATURE_MULTI_AGENT: "true",
      FEATURE_EVAL: "false",
      FEATURE_FLEET: "true"
    });

    expect(flags).toEqual({
      streaming: true,
      subgraphs: false,
      "multi-agent": true,
      eval: false,
      fleet: true
    });
  });
});
