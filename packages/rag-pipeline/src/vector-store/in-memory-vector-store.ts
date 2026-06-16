import type { Chunk, RetrievalResult } from "../types.js";
import type { VectorStore } from "./vector-store.js";

const cosineSimilarity = (a: number[], b: number[]): number => {
  const size = Math.max(a.length, b.length);
  let dot = 0;
  let normA = 0;
  let normB = 0;
  for (let i = 0; i < size; i += 1) {
    const av = a[i] ?? 0;
    const bv = b[i] ?? 0;
    dot += av * bv;
    normA += av * av;
    normB += bv * bv;
  }
  if (normA === 0 || normB === 0) {
    return 0;
  }
  return dot / (Math.sqrt(normA) * Math.sqrt(normB));
};

export class InMemoryVectorStore implements VectorStore {
  private readonly chunks = new Map<string, Chunk>();

  public async upsert(chunks: Chunk[]): Promise<void> {
    for (const chunk of chunks) {
      this.chunks.set(chunk.id, chunk);
    }
  }

  public async search(embedding: number[], topK: number): Promise<RetrievalResult[]> {
    const results: RetrievalResult[] = [...this.chunks.values()]
      .map((chunk) => ({
        chunk,
        score: cosineSimilarity(embedding, chunk.embedding ?? [])
      }))
      .sort((left, right) => right.score - left.score)
      .slice(0, Math.max(0, topK));
    return results;
  }
}
