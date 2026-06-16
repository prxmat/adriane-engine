import type { Chunk, Document, SplitConfig } from "../types.js";

export interface TextSplitter {
  split(doc: Document, config: SplitConfig): Chunk[];
}
