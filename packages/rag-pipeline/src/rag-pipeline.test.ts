import { describe, expect, it } from "vitest";

import type { Document } from "./types.js";
import { MockEmbeddingsAdapter } from "./embeddings/mock-embeddings-adapter.js";
import { Retriever } from "./retriever/retriever.js";
import { RecursiveCharacterSplitter } from "./splitters/recursive-character-splitter.js";
import { InMemoryVectorStore } from "./vector-store/in-memory-vector-store.js";

describe("rag-pipeline", () => {
  it("splits document into chunks", () => {
    const splitter = new RecursiveCharacterSplitter();
    const doc: Document = {
      id: "d1",
      content: "Alpha beta. Gamma delta. Epsilon zeta.",
      metadata: {}
    };
    const chunks = splitter.split(doc, { chunkSize: 15, chunkOverlap: 3 });
    expect(chunks.length).toBeGreaterThan(1);
    expect(chunks[0]?.sourceId).toBe("d1");
  });

  it("embeds using mock adapter", async () => {
    const adapter = new MockEmbeddingsAdapter();
    const vectors = await adapter.embed(["abc", "def"]);
    expect(vectors).toHaveLength(2);
    expect(vectors[0]?.length).toBeGreaterThan(0);
  });

  it("searches with cosine similarity", async () => {
    const store = new InMemoryVectorStore();
    await store.upsert([
      { id: "c1", sourceId: "d1", chunkIndex: 0, content: "alpha", metadata: {}, embedding: [1, 0, 0] },
      { id: "c2", sourceId: "d1", chunkIndex: 1, content: "beta", metadata: {}, embedding: [0, 1, 0] }
    ]);
    const results = await store.search([1, 0, 0], 1);
    expect(results[0]?.chunk.id).toBe("c1");
  });

  it("runs retriever pipeline end-to-end", async () => {
    const adapter = new MockEmbeddingsAdapter();
    const store = new InMemoryVectorStore();
    const chunks = [
      { id: "c1", sourceId: "d1", chunkIndex: 0, content: "critical risk", metadata: {} },
      { id: "c2", sourceId: "d1", chunkIndex: 1, content: "general update", metadata: {} }
    ];
    const vectors = await adapter.embed(chunks.map((chunk) => chunk.content));
    await store.upsert(chunks.map((chunk, index) => ({ ...chunk, embedding: vectors[index] })));
    const retriever = new Retriever(store, adapter, 2);

    const results = await retriever.invoke("critical");
    expect(results.length).toBeGreaterThan(0);
  });
});
