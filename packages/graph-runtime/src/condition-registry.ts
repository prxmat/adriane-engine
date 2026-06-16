import type { ConditionFn, ConditionRegistry } from "./interfaces.js";

export class InMemoryConditionRegistry implements ConditionRegistry {
  private readonly conditions = new Map<string, ConditionFn>();

  public register(name: string, fn: ConditionFn): void {
    this.conditions.set(name, fn);
  }

  public resolve(name: string): ConditionFn | undefined {
    return this.conditions.get(name);
  }
}
