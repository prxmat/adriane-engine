export interface EmbeddingsAdapter {
  embed(texts: string[]): Promise<number[][]>;
}
