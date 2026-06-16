import type { RetryPolicy, RunnableConfig } from "./types.js";

export interface Runnable<TInput, TOutput> {
  invoke(input: TInput, config?: RunnableConfig): Promise<TOutput>;
  stream(input: TInput, config?: RunnableConfig): AsyncIterable<TOutput>;
  batch(inputs: TInput[], config?: RunnableConfig): Promise<TOutput[]>;
  pipe<TNext>(next: Runnable<TOutput, TNext>): Runnable<TInput, TNext>;
  withRetry(policy: RetryPolicy): Runnable<TInput, TOutput>;
  withFallbacks(fallbacks: Runnable<TInput, TOutput>[]): Runnable<TInput, TOutput>;
}
