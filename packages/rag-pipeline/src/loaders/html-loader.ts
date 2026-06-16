import { readFile } from "node:fs/promises";

import type { Document } from "../types.js";
import type { DocumentLoader } from "./document-loader.js";

const stripHtml = (html: string): string => html.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();

const resolveInput = async (input: string): Promise<string> => {
  try {
    return await readFile(input, "utf8");
  } catch {
    return input;
  }
};

export class HtmlLoader implements DocumentLoader {
  public constructor(private readonly input: string) {}

  public async load(): Promise<Document[]> {
    const content = stripHtml(await resolveInput(this.input));
    return [{ id: "html:0", content, metadata: { loader: "html" } }];
  }
}
