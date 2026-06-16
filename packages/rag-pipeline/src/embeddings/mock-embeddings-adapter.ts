import type { EmbeddingsAdapter } from "./embeddings-adapter.js";

const toVector = (text: string): number[] => {
  const counts = [0, 0, 0, 0];
  for (const char of text) {
    const idx = char.charCodeAt(0) % counts.length;
    const current = counts[idx] ?? 0;
    counts[idx] = current + 1;
  }
  return counts;
};

export class MockEmbeddingsAdapter implements EmbeddingsAdapter {
  public async embed(texts: string[]): Promise<number[][]> {
    return texts.map((text) => toVector(text));
  }
}
