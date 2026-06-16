export type StepBudget = {
  maxSteps: number;
  currentSteps: number;
};

export class StepBudgetExceededError extends Error {
  public readonly maxSteps: number;
  public readonly currentSteps: number;

  public constructor(budget: StepBudget) {
    super(`Step budget exceeded: ${budget.currentSteps}/${budget.maxSteps}.`);
    this.name = "StepBudgetExceededError";
    this.maxSteps = budget.maxSteps;
    this.currentSteps = budget.currentSteps;
  }
}

export const checkBudget = (budget: StepBudget): void => {
  if (budget.currentSteps > budget.maxSteps) {
    throw new StepBudgetExceededError(budget);
  }
};
