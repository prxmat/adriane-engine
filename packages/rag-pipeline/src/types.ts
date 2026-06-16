export type Document = {
  id: string;
  content: string;
  metadata: Record<string, unknown>;
  embedding?: number[];
};

export type Chunk = Document & {
  sourceId: string;
  chunkIndex: number;
};

export type SplitConfig = {
  chunkSize: number;
  chunkOverlap: number;
};

export type RetrievalResult = {
  chunk: Chunk;
  score: number;
};
