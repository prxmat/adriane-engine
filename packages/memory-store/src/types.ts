export type MemoryNamespace = string[];
export type MemoryKey = string;

export type MemoryItem = {
  namespace: MemoryNamespace;
  key: MemoryKey;
  value: unknown;
  createdAt: string;
  updatedAt: string;
  embedding?: number[];
};
