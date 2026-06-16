import type { BaseStore } from "./interfaces.js";
import type { MemoryItem, MemoryKey, MemoryNamespace } from "./types.js";

export class PgStore implements BaseStore {
  public async get(namespace: MemoryNamespace, key: MemoryKey): Promise<MemoryItem | undefined> {
    void namespace;
    void key;
    throw new Error("PgStore.get is not implemented yet.");
  }

  public async put(namespace: MemoryNamespace, key: MemoryKey, value: unknown): Promise<MemoryItem> {
    void namespace;
    void key;
    void value;
    throw new Error("PgStore.put is not implemented yet.");
  }

  public async delete(namespace: MemoryNamespace, key: MemoryKey): Promise<void> {
    void namespace;
    void key;
    throw new Error("PgStore.delete is not implemented yet.");
  }

  public async search(namespace: MemoryNamespace, query: string, topK: number): Promise<MemoryItem[]> {
    void namespace;
    void query;
    void topK;
    throw new Error("PgStore.search with pgvector is not implemented yet.");
  }

  public async list(namespace: MemoryNamespace, prefix?: string): Promise<MemoryItem[]> {
    void namespace;
    void prefix;
    throw new Error("PgStore.list is not implemented yet.");
  }
}
