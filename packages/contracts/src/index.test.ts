import { describe, expect, it } from "vitest";

import * as contracts from "./index.js";

describe("@adriane-ai/contracts exports", () => {
  it("exposes contract modules", () => {
    expect(contracts).toBeTypeOf("object");
    expect(Object.keys(contracts).length).toBeGreaterThan(0);
  });
});
