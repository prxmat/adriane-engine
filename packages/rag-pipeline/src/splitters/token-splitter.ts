import type { Chunk, Document, SplitConfig } from "../types.js";
import type { TextSplitter } from "./text-splitter.js";

const estimateTokens = (text: string): string[] => text.split(/\s+/).filter((token) => token.length > 0);

export class TokenSplitter implements TextSplitter {
  public split(doc: Document, config: SplitConfig): Chunk[] {
    const tokens = estimateTokens(doc.content);
    const size = Math.max(1, config.chunkSize);
    const overlap = Math.max(0, config.chunkOverlap);
    const chunks: Chunk[] = [];
    let index = 0;
    for (let start = 0; start < tokens.length; start += Math.max(1, size - overlap)) {
      const slice = tokens.slice(start, start + size);
      if (slice.length === 0) {
        continue;
      }
      chunks.push({
        ...doc,
        sourceId: doc.id,
        chunkIndex: index,
        id: `${doc.id}:chunk:${index}`,
        content: slice.join(" ")
      });
      index += 1;
      if (start + size >= tokens.length) {
        break;
      }
    }
    return chunks;
  }
}
