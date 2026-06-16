import type { Runnable } from "./interfaces.js";
import type { RetryPolicy, RunnableConfig } from "./types.js";
import { RunnableLambda } from "./runnable-lambda.js";
import { RunnableSequence } from "./runnable-sequence.js";

export class RunnablePassthrough<T> implements Runnable<T, T> {
  public async invoke(input: T, config?: RunnableConfig): Promise<T> {
    void config;
    return input;
  }

  public async *stream(input: T, config?: RunnableConfig): AsyncIterable<T> {
    yield await this.invoke(input, config);
  }

  public async batch(inputs: T[], config?: RunnableConfig): Promise<T[]> {
    void config;
    return inputs;
  }

  public pipe<TNext>(next: Runnable<T, TNext>): Runnable<T, TNext> {
    return new RunnableSequence<T, TNext>([this as Runnable<unknown, unknown>, next as Runnable<unknown, unknown>]);
  }

  public withRetry(policy: RetryPolicy): Runnable<T, T> {
    return new RunnableLambda<T, T>((input: T, config?: RunnableConfig) => this.invoke(input, config)).withRetry(
      policy
    );
  }

  public withFallbacks(fallbacks: Runnable<T, T>[]): Runnable<T, T> {
    return new RunnableLambda<T, T>((input: T, config?: RunnableConfig) => this.invoke(input, config)).withFallbacks(
      fallbacks
    );
  }
}
