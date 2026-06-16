import type { Runnable } from "../../../runnable/src/interfaces.js";
import type { RunnableConfig } from "../../../runnable/src/types.js";
import { RunnableLambda } from "../../../runnable/src/runnable-lambda.js";

import type { EmbeddingsAdapter } from "../embeddings/embeddings-adapter.js";
import type { RetrievalResult } from "../types.js";
import type { VectorStore } from "../vector-store/vector-store.js";

export class Retriever implements Runnable<string, RetrievalResult[]> {
  public constructor(
    private readonly vectorStore: VectorStore,
    private readonly embeddings: EmbeddingsAdapter,
    private readonly topK = 5
  ) {}

  public async invoke(input: string, config?: RunnableConfig): Promise<RetrievalResult[]> {
    void config;
    const [embedding] = await this.embeddings.embed([input]);
    if (embedding === undefined) {
      return [];
    }
    return this.vectorStore.search(embedding, this.topK);
  }

  public async *stream(input: string, config?: RunnableConfig): AsyncIterable<RetrievalResult[]> {
    yield await this.invoke(input, config);
  }

  public async batch(inputs: string[], config?: RunnableConfig): Promise<RetrievalResult[][]> {
    return Promise.all(inputs.map((input) => this.invoke(input, config)));
  }

  public pipe<TNext>(next: Runnable<RetrievalResult[], TNext>): Runnable<string, TNext> {
    return new RunnableLambda<string, RetrievalResult[]>((input, config) => this.invoke(input, config)).pipe(next);
  }

  public withRetry(policy: { maxAttempts: number; backoffMs?: number }): Runnable<string, RetrievalResult[]> {
    return new RunnableLambda<string, RetrievalResult[]>((input, config) => this.invoke(input, config)).withRetry(
      policy
    );
  }

  public withFallbacks(fallbacks: Runnable<string, RetrievalResult[]>[]): Runnable<string, RetrievalResult[]> {
    return new RunnableLambda<string, RetrievalResult[]>((input, config) =>
      this.invoke(input, config)
    ).withFallbacks(fallbacks);
  }
}
