import type { RetrievalResult } from "../types.js";

export interface Reranker {
  rerank(query: string, results: RetrievalResult[], topK: number): Promise<RetrievalResult[]>;
}
