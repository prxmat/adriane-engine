/**
 * Real text embeddings as an exported SDK helper (NOT a catalog component kind).
 *
 * {@link createEmbeddings} returns an {@link Embeddings} whose `embed` turns a batch of
 * texts into dense vectors. The default transport POSTs to the Mistral embeddings API
 * (`/embeddings` with `{ model, input }` and a `Bearer` key), parsing `data[].embedding`.
 * The {@link CreateEmbeddingsOptions.transport} hook overrides the network call so a test
 * can return deterministic vectors with no real network. This is the embedding backbone
 * behind {@link import("./semantic-retriever.js").semanticRetriever}.
 *
 * ```ts
 * import { createEmbeddings } from "@adriane-ai/graph-sdk";
 *
 * const embeddings = createEmbeddings({ apiKey: process.env.MISTRAL_API_KEY });
 * const [a, b] = await embeddings.embed(["hello", "world"]);
 * ```
 */

/** An embedder: turn a batch of texts into one dense vector each (order-preserving). */
export type Embeddings = {
  /** Embed `texts` into a `number[][]` of the same length and order. */
  embed(texts: string[]): Promise<number[][]>;
};

/**
 * The transport an embeddings client posts through: it receives the assembled request
 * body and must resolve to the parsed JSON response (the `{ data: [{ embedding }] }`
 * shape the Mistral embeddings API returns). The real default builds this from `fetch`;
 * a test injects a fake to stay offline.
 */
export type EmbeddingsTransport = (body: EmbeddingsRequestBody) => Promise<unknown> | unknown;

/** The request body POSTed to the embeddings endpoint (`{ model, input }`). */
export type EmbeddingsRequestBody = {
  model: string;
  input: string[];
};

/** Options for {@link createEmbeddings}. */
export type CreateEmbeddingsOptions = {
  /** The embeddings provider. Only `"mistral"` is wired today (the default). */
  provider?: "mistral";
  /** API key. Defaults to `process.env.MISTRAL_API_KEY`. Required unless `transport` is injected. */
  apiKey?: string;
  /** Embedding model. Defaults to `"mistral-embed"`. */
  model?: string;
  /** API base URL. Defaults to `"https://api.mistral.ai/v1"`. */
  baseUrl?: string;
  /**
   * An injectable transport overriding the default `fetch`-based call. Receives the
   * request body and returns the parsed JSON response. Inject a fake to keep a test
   * offline (or to point at a stub) — when set, no API key is required.
   */
  transport?: EmbeddingsTransport;
};

/** The default Mistral embeddings model. */
const DEFAULT_MODEL = "mistral-embed";
/** The default Mistral API base URL. */
const DEFAULT_BASE_URL = "https://api.mistral.ai/v1";

/** Raised when no API key and no transport were supplied, so a real call is impossible. */
export class MissingEmbeddingsKeyError extends Error {
  public constructor() {
    super(
      "createEmbeddings: no API key (set `apiKey` or MISTRAL_API_KEY) and no `transport` injected; " +
        "supply one to make a real call, or inject `transport` for offline use."
    );
    this.name = "MissingEmbeddingsKeyError";
  }
}

/** Raised when the embeddings response doesn't carry the expected `data[].embedding` shape. */
export class EmbeddingsResponseError extends Error {
  public constructor(detail: string) {
    super(`createEmbeddings: malformed embeddings response: ${detail}`);
    this.name = "EmbeddingsResponseError";
  }
}

/**
 * Parse a `{ data: [{ embedding: number[] }, …] }` response into a `number[][]`,
 * preserving order. Throws an {@link EmbeddingsResponseError} when the shape is wrong.
 */
const parseEmbeddingsResponse = (parsed: unknown): number[][] => {
  if (typeof parsed !== "object" || parsed === null) {
    throw new EmbeddingsResponseError("response is not an object");
  }
  const data = (parsed as { data?: unknown }).data;
  if (!Array.isArray(data)) {
    throw new EmbeddingsResponseError("`data` is not an array");
  }
  return data.map((entry, index) => {
    const embedding =
      typeof entry === "object" && entry !== null
        ? (entry as { embedding?: unknown }).embedding
        : undefined;
    if (!Array.isArray(embedding) || !embedding.every((n) => typeof n === "number")) {
      throw new EmbeddingsResponseError(`\`data[${index}].embedding\` is not a number[]`);
    }
    return embedding as number[];
  });
};

/**
 * The default `fetch`-based transport: POST `{ model, input }` to
 * `{baseUrl}/embeddings` with an `Authorization: Bearer <key>` header, returning the
 * parsed JSON. Throws on a non-2xx status. Only built when no `transport` is injected.
 */
const createMistralTransport =
  (apiKey: string, baseUrl: string): EmbeddingsTransport =>
  async (body) => {
    const response = await globalThis.fetch(`${baseUrl}/embeddings`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${apiKey}`
      },
      body: JSON.stringify(body)
    });
    const text = await response.text();
    if (!response.ok) {
      throw new EmbeddingsResponseError(`status ${response.status}: ${text}`);
    }
    return JSON.parse(text) as unknown;
  };

/**
 * Create an {@link Embeddings} client. With the default transport it POSTs to the
 * Mistral embeddings API (`{baseUrl||'https://api.mistral.ai/v1'}/embeddings`) with
 * `{ model: model||'mistral-embed', input: texts }` and `Authorization: Bearer
 * (apiKey || process.env.MISTRAL_API_KEY)`, parsing `data[].embedding`. Inject
 * `transport` to override that for offline tests. Throws {@link MissingEmbeddingsKeyError}
 * when neither a key nor a transport is available.
 */
export const createEmbeddings = (options: CreateEmbeddingsOptions = {}): Embeddings => {
  const model = options.model ?? DEFAULT_MODEL;
  const baseUrl = options.baseUrl ?? DEFAULT_BASE_URL;

  let transport: EmbeddingsTransport;
  if (options.transport !== undefined) {
    transport = options.transport;
  } else {
    const apiKey = options.apiKey ?? process.env.MISTRAL_API_KEY;
    if (apiKey === undefined || apiKey.length === 0) {
      throw new MissingEmbeddingsKeyError();
    }
    transport = createMistralTransport(apiKey, baseUrl);
  }

  return {
    async embed(texts) {
      if (texts.length === 0) {
        return [];
      }
      const parsed = await transport({ model, input: texts });
      return parseEmbeddingsResponse(parsed);
    }
  };
};
