export class RecursionLimitError extends Error {
  public constructor(limit: number) {
    super(`Recursion limit exceeded (${limit}).`);
    this.name = "RecursionLimitError";
  }
}
