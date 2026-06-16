import type { LLMGateway } from "../../../llm-gateway/src/interfaces.js";
import type { RetrievalResult } from "../types.js";
import type { Reranker } from "./reranker.js";

export class LLMReranker implements Reranker {
  public constructor(private readonly gateway: LLMGateway) {}

  public async rerank(query: string, results: RetrievalResult[], topK: number): Promise<RetrievalResult[]> {
    if (results.length === 0) {
      return [];
    }
    const scored = await Promise.all(
      results.map(async (result) => {
        const response = await this.gateway.complete({
          provider: "openai",
          model: "mock-reranker",
          messages: [
            {
              role: "user",
              content: `Score relevance from 0 to 1.\nQuery: ${query}\nText: ${result.chunk.content}`
            }
          ]
        });
        const parsed = Number.parseFloat(response.content);
        return {
          ...result,
          score: Number.isFinite(parsed) ? parsed : result.score
        };
      })
    );
    return scored.sort((a, b) => b.score - a.score).slice(0, Math.max(0, topK));
  }
}
