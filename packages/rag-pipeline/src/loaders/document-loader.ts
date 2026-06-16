import type { Document } from "../types.js";

export interface DocumentLoader {
  load(): Promise<Document[]>;
}
