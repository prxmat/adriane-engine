import { describe, expect, it } from "vitest";

import { checkBudget, StepBudgetExceededError } from "./step-budget.js";

describe("step budget", () => {
  it("throws when budget is exceeded", () => {
    expect(() => checkBudget({ maxSteps: 1, currentSteps: 2 })).toThrow(StepBudgetExceededError);
  });
});
