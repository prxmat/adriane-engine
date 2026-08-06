import { describe, expect, it } from "vitest";

import { IngestKbDocumentsDtoSchema, IngestOkfBundleDtoSchema } from "./knowledge.js";

describe("IngestKbDocumentsDtoSchema (issue #453 — bounded documents array)", () => {
  it("accepts a batch within the 500-document cap", () => {
    const documents = Array.from({ length: 500 }, (_, i) => ({ content: `doc-${i}` }));
    expect(() => IngestKbDocumentsDtoSchema.parse({ documents })).not.toThrow();
  });

  it("rejects a batch over the 500-document cap", () => {
    const documents = Array.from({ length: 501 }, (_, i) => ({ content: `doc-${i}` }));
    expect(() => IngestKbDocumentsDtoSchema.parse({ documents })).toThrow();
  });

  it("still rejects an empty batch (pre-existing .min(1))", () => {
    expect(() => IngestKbDocumentsDtoSchema.parse({ documents: [] })).toThrow();
  });
});

describe("IngestOkfBundleDtoSchema (issue #453 — bounded files array)", () => {
  it("rejects a bundle over the 500-file cap", () => {
    const files = Array.from({ length: 501 }, (_, i) => ({ path: `f${i}.md`, content: "x" }));
    expect(() => IngestOkfBundleDtoSchema.parse({ files })).toThrow();
  });
});
