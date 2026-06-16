import type { Chunk, RetrievalResult } from "../types.js";

export interface VectorStore {
  upsert(chunks: Chunk[]): Promise<void>;
  search(embedding: number[], topK: number): Promise<RetrievalResult[]>;
}
