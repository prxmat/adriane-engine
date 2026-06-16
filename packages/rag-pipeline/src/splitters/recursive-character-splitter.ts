import type { Chunk, Document, SplitConfig } from "../types.js";
import type { TextSplitter } from "./text-splitter.js";

const splitSentences = (text: string): string[] =>
  text
    .split(/(?<=[.!?])\s+/)
    .flatMap((sentence) => sentence.split(/\n{2,}/))
    .map((part) => part.trim())
    .filter((part) => part.length > 0);

export class RecursiveCharacterSplitter implements TextSplitter {
  public split(doc: Document, config: SplitConfig): Chunk[] {
    const units = splitSentences(doc.content);
    const chunks: Chunk[] = [];
    let current = "";
    let chunkIndex = 0;
    for (const unit of units) {
      const candidate = current.length === 0 ? unit : `${current} ${unit}`;
      if (candidate.length > config.chunkSize && current.length > 0) {
        chunks.push({
          ...doc,
          sourceId: doc.id,
          chunkIndex,
          id: `${doc.id}:chunk:${chunkIndex}`,
          content: current
        });
        chunkIndex += 1;
        const overlap = Math.max(0, config.chunkOverlap);
        current = `${current.slice(Math.max(0, current.length - overlap))} ${unit}`.trim();
      } else {
        current = candidate;
      }
    }
    if (current.length > 0) {
      chunks.push({
        ...doc,
        sourceId: doc.id,
        chunkIndex,
        id: `${doc.id}:chunk:${chunkIndex}`,
        content: current
      });
    }
    return chunks;
  }
}
