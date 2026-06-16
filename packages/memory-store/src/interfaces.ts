import type { MemoryItem, MemoryKey, MemoryNamespace } from "./types.js";

export interface BaseStore {
  get(namespace: MemoryNamespace, key: MemoryKey): Promise<MemoryItem | undefined>;
  put(namespace: MemoryNamespace, key: MemoryKey, value: unknown): Promise<MemoryItem>;
  delete(namespace: MemoryNamespace, key: MemoryKey): Promise<void>;
  search(namespace: MemoryNamespace, query: string, topK: number): Promise<MemoryItem[]>;
  list(namespace: MemoryNamespace, prefix?: string): Promise<MemoryItem[]>;
}
