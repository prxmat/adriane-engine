import type { Runnable } from "./interfaces.js";
import type { RetryPolicy, RunnableConfig } from "./types.js";
import { RunnableLambda } from "./runnable-lambda.js";

export class RunnableSequence<TInput, TOutput> implements Runnable<TInput, TOutput> {
  public constructor(private readonly runnables: Runnable<unknown, unknown>[]) {}

  public async invoke(input: TInput, config?: RunnableConfig): Promise<TOutput> {
    let current: unknown = input;
    for (const runnable of this.runnables) {
      current = await runnable.invoke(current, config);
    }
    return current as TOutput;
  }

  public async *stream(input: TInput, config?: RunnableConfig): AsyncIterable<TOutput> {
    yield await this.invoke(input, config);
  }

  public async batch(inputs: TInput[], config?: RunnableConfig): Promise<TOutput[]> {
    return Promise.all(inputs.map((input) => this.invoke(input, config)));
  }

  public pipe<TNext>(next: Runnable<TOutput, TNext>): Runnable<TInput, TNext> {
    return new RunnableSequence<TInput, TNext>([...this.runnables, next as Runnable<unknown, unknown>]);
  }

  public withRetry(policy: RetryPolicy): Runnable<TInput, TOutput> {
    return new RunnableLambda<TInput, TOutput>((input: TInput, config?: RunnableConfig) =>
      this.invoke(input, config)
    ).withRetry(policy);
  }

  public withFallbacks(fallbacks: Runnable<TInput, TOutput>[]): Runnable<TInput, TOutput> {
    return new RunnableLambda<TInput, TOutput>((input: TInput, config?: RunnableConfig) =>
      this.invoke(input, config)
    ).withFallbacks(fallbacks);
  }
}
