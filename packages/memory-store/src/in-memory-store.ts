import type { BaseStore } from "./interfaces.js";
import type { MemoryItem, MemoryKey, MemoryNamespace } from "./types.js";

const now = (): string => new Date().toISOString();
const nsToKey = (namespace: MemoryNamespace): string => namespace.join("|");

export class InMemoryStore implements BaseStore {
  private readonly map = new Map<string, MemoryItem>();

  public async get(namespace: MemoryNamespace, key: MemoryKey): Promise<MemoryItem | undefined> {
    return this.map.get(`${nsToKey(namespace)}:${key}`);
  }

  public async put(namespace: MemoryNamespace, key: MemoryKey, value: unknown): Promise<MemoryItem> {
    const mapKey = `${nsToKey(namespace)}:${key}`;
    const existing = this.map.get(mapKey);
    const item: MemoryItem = {
      namespace,
      key,
      value,
      createdAt: existing?.createdAt ?? now(),
      updatedAt: now()
    };
    this.map.set(mapKey, item);
    return item;
  }

  public async delete(namespace: MemoryNamespace, key: MemoryKey): Promise<void> {
    this.map.delete(`${nsToKey(namespace)}:${key}`);
  }

  public async search(namespace: MemoryNamespace, query: string, topK: number): Promise<MemoryItem[]> {
    const q = query.toLowerCase();
    return [...this.map.values()]
      .filter((item) => nsToKey(item.namespace) === nsToKey(namespace))
      .filter((item) => JSON.stringify(item.value).toLowerCase().includes(q) || item.key.toLowerCase().includes(q))
      .slice(0, Math.max(0, topK));
  }

  public async list(namespace: MemoryNamespace, prefix?: string): Promise<MemoryItem[]> {
    return [...this.map.values()].filter(
      (item) =>
        nsToKey(item.namespace) === nsToKey(namespace) &&
        (prefix === undefined || item.key.startsWith(prefix))
    );
  }
}
