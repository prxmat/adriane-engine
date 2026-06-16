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

export class CsvLoader implements DocumentLoader {
  public constructor(private readonly input: string) {}

  public async load(): Promise<Document[]> {
    const csv = await resolveInput(this.input);
    const lines = csv.split(/\r?\n/).filter((line) => line.trim().length > 0);
    return lines.map((line, index) => ({
      id: `csv:${index}`,
      content: line,
      metadata: { loader: "csv", row: index }
    }));
  }
}
