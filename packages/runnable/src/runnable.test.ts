import { describe, expect, it } from "vitest";

import { RunnableLambda } from "./runnable-lambda.js";
import { RunnableParallel } from "./runnable-parallel.js";

describe("Runnable", () => {
  it("supports pipe composition", async () => {
    const add1 = new RunnableLambda<number, number>(async (input) => input + 1);
    const times2 = new RunnableLambda<number, number>(async (input) => input * 2);

    const result = await add1.pipe(times2).invoke(2);
    expect(result).toBe(6);
  });

  it("supports parallel execution", async () => {
    const parallel = new RunnableParallel<number, { add: number; mul: number }>({
      add: new RunnableLambda<number, number>(async (input) => input + 1),
      mul: new RunnableLambda<number, number>(async (input) => input * 3)
    });

    const result = await parallel.invoke(4);
    expect(result).toEqual({ add: 5, mul: 12 });
  });

  it("supports withRetry", async () => {
    let attempts = 0;
    const flaky = new RunnableLambda<number, number>(async (input) => {
      attempts += 1;
      if (attempts < 3) {
        throw new Error("try again");
      }
      return input;
    });

    const result = await flaky.withRetry({ maxAttempts: 3 }).invoke(7);
    expect(result).toBe(7);
    expect(attempts).toBe(3);
  });

  it("supports withFallbacks", async () => {
    const failing = new RunnableLambda<number, number>(async () => {
      throw new Error("boom");
    });
    const fallback = new RunnableLambda<number, number>(async (input) => input + 10);

    const result = await failing.withFallbacks([fallback]).invoke(2);
    expect(result).toBe(12);
  });
});
