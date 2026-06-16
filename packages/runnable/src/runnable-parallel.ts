import type { Runnable } from "./interfaces.js";
import type { RetryPolicy, RunnableConfig } from "./types.js";
import { RunnableLambda } from "./runnable-lambda.js";
import { RunnableSequence } from "./runnable-sequence.js";

export class RunnableParallel<TInput, TOutputMap extends Record<string, unknown>>
  implements Runnable<TInput, TOutputMap>
{
  public constructor(private readonly branches: Record<string, Runnable<TInput, unknown>>) {}

  public async invoke(input: TInput, config?: RunnableConfig): Promise<TOutputMap> {
    const entries = await Promise.all(
      Object.entries(this.branches).map(async ([key, runnable]) => [key, await runnable.invoke(input, config)] as const)
    );
    return Object.fromEntries(entries) as TOutputMap;
  }

  public async *stream(input: TInput, config?: RunnableConfig): AsyncIterable<TOutputMap> {
    yield await this.invoke(input, config);
  }

  public async batch(inputs: TInput[], config?: RunnableConfig): Promise<TOutputMap[]> {
    return Promise.all(inputs.map((input) => this.invoke(input, config)));
  }

  public pipe<TNext>(next: Runnable<TOutputMap, TNext>): Runnable<TInput, TNext> {
    return new RunnableSequence<TInput, TNext>([this as Runnable<unknown, unknown>, next as Runnable<unknown, unknown>]);
  }

  public withRetry(policy: RetryPolicy): Runnable<TInput, TOutputMap> {
    return new RunnableLambda<TInput, TOutputMap>((input: TInput, config?: RunnableConfig) =>
      this.invoke(input, config)
    ).withRetry(policy);
  }

  public withFallbacks(fallbacks: Runnable<TInput, TOutputMap>[]): Runnable<TInput, TOutputMap> {
    return new RunnableLambda<TInput, TOutputMap>((input: TInput, config?: RunnableConfig) =>
      this.invoke(input, config)
    ).withFallbacks(fallbacks);
  }
}
