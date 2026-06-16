import { readFile } from "node:fs/promises";

import type { Document } from "../types.js";
import type { DocumentLoader } from "./document-loader.js";

const resolveInput = async (input: string): Promise<string> => {
  try {
    return await readFile(input, "utf8");
  } catch {
    return input;
  }
};

export class JsonLoader implements DocumentLoader {
  public constructor(private readonly input: string) {}

  public async load(): Promise<Document[]> {
    const raw = await resolveInput(this.input);
    const parsed = JSON.parse(raw) as unknown;
    const rows = Array.isArray(parsed) ? parsed : [parsed];
    return rows.map((row, index) => ({
      id: `json:${index}`,
      content: JSON.stringify(row),
      metadata: { loader: "json", index }
    }));
  }
}
