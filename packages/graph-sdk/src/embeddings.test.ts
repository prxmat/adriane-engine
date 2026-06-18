import { describe, expect, it } from "vitest";

import {
  createEmbeddings,
  EmbeddingsResponseError,
  MissingEmbeddingsKeyError,
  type EmbeddingsRequestBody
} from "./embeddings.js";

describe("@adriane-ai/graph-sdk — createEmbeddings (injected transport, offline)", () => {
  it("returns the parsed vectors from an injected transport, preserving order", async () => {
    const seen: EmbeddingsRequestBody[] = [];
    const embeddings = createEmbeddings({
      transport: (body) => {
        seen.push(body);
        return {
          data: body.input.map((text, i) => ({ embedding: [text.length, i] }))
        };
      }
    });

    const vectors = await embeddings.embed(["a", "bb", "ccc"]);
    expect(vectors).toEqual([
      [1, 0],
      [2, 1],
      [3, 2]
    ]);
    // Default model + the texts as `input`.
    expect(seen).toEqual([{ model: "mistral-embed", input: ["a", "bb", "ccc"] }]);
  });

  it("honours an injected model and never calls the transport for an empty batch", async () => {
    let called = false;
    const embeddings = createEmbeddings({
      model: "custom-embed",
      transport: (body) => {
        called = true;
        return { data: body.input.map(() => ({ embedding: [0] })) };
      }
    });

    expect(await embeddings.embed([])).toEqual([]);
    expect(called).toBe(false);

    await embeddings.embed(["x"]);
    expect(called).toBe(true);
  });

  it("throws MissingEmbeddingsKeyError when no key and no transport are available", () => {
    const savedKey = process.env.MISTRAL_API_KEY;
    delete process.env.MISTRAL_API_KEY;
    try {
      expect(() => createEmbeddings()).toThrow(MissingEmbeddingsKeyError);
    } finally {
      if (savedKey === undefined) {
        delete process.env.MISTRAL_API_KEY;
      } else {
        process.env.MISTRAL_API_KEY = savedKey;
      }
    }
  });

  it("does not require a key when a transport is injected", () => {
    const savedKey = process.env.MISTRAL_API_KEY;
    delete process.env.MISTRAL_API_KEY;
    try {
      expect(() =>
        createEmbeddings({ transport: () => ({ data: [] }) })
      ).not.toThrow();
    } finally {
      if (savedKey === undefined) {
        delete process.env.MISTRAL_API_KEY;
      } else {
        process.env.MISTRAL_API_KEY = savedKey;
      }
    }
  });

  it("throws EmbeddingsResponseError on a malformed response shape", async () => {
    const embeddings = createEmbeddings({ transport: () => ({ notData: true }) });
    await expect(embeddings.embed(["x"])).rejects.toBeInstanceOf(EmbeddingsResponseError);
  });
});
