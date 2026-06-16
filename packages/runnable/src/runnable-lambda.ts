import type { Runnable } from "./interfaces.js";
import type { RetryPolicy, RunnableConfig } from "./types.js";
import { RunnableSequence } from "./runnable-sequence.js";

type LambdaFn<TInput, TOutput> = (input: TInput, config?: RunnableConfig) => Promise<TOutput>;

const sleep = async (ms: number): Promise<void> => {
  if (ms <= 0) {
    return;
  }
  await new Promise((resolve) => setTimeout(resolve, ms));
};

export class RunnableLambda<TInput, TOutput> implements Runnable<TInput, TOutput> {
  public constructor(private readonly fn: LambdaFn<TInput, TOutput>) {}

  public async invoke(input: TInput, config?: RunnableConfig): Promise<TOutput> {
    const callbacks = config?.callbacks ?? [];
    for (const cb of callbacks) {
      await cb.onStart?.(input);
    }
    try {
      const output = await this.fn(input, config);
      for (const cb of callbacks) {
        await cb.onEnd?.(output);
      }
      return output;
    } catch (error) {
      for (const cb of callbacks) {
        await cb.onError?.(error);
      }
      throw error;
    }
  }

  public async *stream(input: TInput, config?: RunnableConfig): AsyncIterable<TOutput> {
    yield await this.invoke(input, config);
  }

  public async batch(inputs: TInput[], config?: RunnableConfig): Promise<TOutput[]> {
    return Promise.all(inputs.map((input) => this.invoke(input, config)));
  }

  public pipe<TNext>(next: Runnable<TOutput, TNext>): Runnable<TInput, TNext> {
    return new RunnableSequence<TInput, TNext>([this, next]);
  }

  public withRetry(policy: RetryPolicy): Runnable<TInput, TOutput> {
    return new RunnableLambda<TInput, TOutput>(async (input, config) => {
      const maxAttempts = Math.max(1, policy.maxAttempts);
      let lastError: unknown;
      for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
        try {
          return await this.invoke(input, config);
        } catch (error) {
          lastError = error;
          if (attempt < maxAttempts) {
            await sleep(policy.backoffMs ?? 0);
          }
        }
      }
      throw lastError;
    });
  }

  public withFallbacks(fallbacks: Runnable<TInput, TOutput>[]): Runnable<TInput, TOutput> {
    return new RunnableLambda<TInput, TOutput>(async (input, config) => {
      const candidates: Runnable<TInput, TOutput>[] = [this, ...fallbacks];
      let lastError: unknown;
      for (const runnable of candidates) {
        try {
          return await runnable.invoke(input, config);
        } catch (error) {
          lastError = error;
        }
      }
      throw lastError;
    });
  }
}
