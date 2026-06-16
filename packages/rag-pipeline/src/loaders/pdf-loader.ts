import { readFile } from "node:fs/promises";

import type { Document } from "../types.js";
import type { DocumentLoader } from "./document-loader.js";

const resolveInput = async (input: string): Promise<string> => {
  if (input.includes("\n") || input.includes(" ")) {
    return input;
  }
  try {
    return await readFile(input, "utf8");
  } catch {
    return input;
  }
};

export class PdfLoader implements DocumentLoader {
  public constructor(private readonly input: string) {}

  public async load(): Promise<Document[]> {
    const content = await resolveInput(this.input);
    return [{ id: "pdf:0", content, metadata: { loader: "pdf" } }];
  }
}
