import { describe, expect, it } from "vitest";

import { createSwarmHandoff, isSwarmHandoff } from "./swarm.js";

describe("swarm handoff", () => {
  it("creates and validates a handoff payload", () => {
    const handoff = createSwarmHandoff("agent-next" as never, "needs specialist");
    expect(isSwarmHandoff(handoff)).toBe(true);
    expect(handoff.goto).toBe("agent-next");
    expect(handoff.update.reason).toBe("needs specialist");
  });
});
