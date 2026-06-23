/**
 * `@adriane-ai/model-core` — the shared base for Adriane's per-provider model packages
 * (`@adriane-ai/model-openai`, `-anthropic`, `-gemini`, `-mistral`), ADR 0031.
 *
 * Adriane runs on **one Rust engine**. A model package is a thin SDK **overlay**: it declares
 * a serializable {@link ModelSpec} (provider + model + tier) you pass to `agentNode({ model })`
 * — the Rust engine executes the graph — and it can be called standalone via {@link Model.invoke}
 * / {@link Model.stream}, which route a one-shot through the Rust gateway over the napi seam.
 * The HTTP happens in Rust: no TS provider client, no second engine, one consistent behaviour.
 */

import { createRequire } from "node:module";

/** Provider slugs the compiled-in Rust adapters understand (mirrors the Rust `LlmProvider`). */
export type ProviderSlug =
  | "openai"
  | "anthropic"
  | "google"
  | "mistral"
  | "openrouter"
  | "minimax"
  | "huggingface"
  | "ollama"
  | "lmstudio";

const KNOWN_PROVIDERS: ReadonlySet<string> = new Set<ProviderSlug>([
  "openai",
  "anthropic",
  "google",
  "mistral",
  "openrouter",
  "minimax",
  "huggingface",
  "ollama",
  "lmstudio"
]);

/** Capability tier (mirrors the Rust `ModelPolicy` tiers). */
export type ModelTier = "fast" | "balanced" | "frontier" | "creative";

/**
 * A serializable model declaration — the authoring surface that round-trips through both the
 * napi (TS) and pyo3 (Python) seams. `baseURL`/`apiKeyEnv` are carried for the OpenAI-compatible
 * escape hatch (honoured on the graph path via provider keys/env; standalone-invoke wiring is a
 * follow-up).
 */
export type ModelSpec = {
  provider: ProviderSlug;
  model?: string;
  tier?: ModelTier;
  baseURL?: string;
  apiKeyEnv?: string;
};

/** Fails loudly on an unknown provider slug (defuses the Rust catch-all silent-Anthropic). */
export function assertKnownProvider(provider: string): asserts provider is ProviderSlug {
  if (!KNOWN_PROVIDERS.has(provider)) {
    throw new Error(
      `Unknown provider "${provider}". Known: ${[...KNOWN_PROVIDERS].join(", ")}. ` +
        `For a custom OpenAI-compatible endpoint use openaiCompatible({ baseURL }).`
    );
  }
}

/** A chat turn for {@link Model.invoke}. */
export type ChatMessage = { role: "system" | "user" | "assistant"; content: string };

/** Options for a one-shot completion. */
export type InvokeOptions = { maxTokens?: number; temperature?: number };

/** Token usage on a {@link ModelResponse}. */
export type ModelUsage = {
  promptTokens: number;
  completionTokens: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
};

/** The result of a one-shot completion (the serialized Rust `LlmResponse`). */
export type ModelResponse = {
  content: string;
  toolCalls?: Array<{ id: string; name: string; input: unknown }>;
  stopReason?: string;
  usage: ModelUsage;
  model: string;
  provider: ProviderSlug;
};

type NapiLlm = { llmComplete(requestJson: string, providerKeysJson: string): Promise<string> };

let cachedNapi: NapiLlm | null | undefined;

function loadNapi(): NapiLlm {
  if (cachedNapi === undefined) {
    try {
      const requireFn = createRequire(import.meta.url);
      const mod = requireFn("@adriane-ai/napi") as Record<string, unknown>;
      cachedNapi = typeof mod.llmComplete === "function" ? (mod as unknown as NapiLlm) : null;
    } catch {
      cachedNapi = null;
    }
  }
  if (!cachedNapi) {
    throw new Error(
      "@adriane-ai/napi (the Rust engine) is not available — Model.invoke()/stream() need it. " +
        "Build it: bash scripts/build-napi.sh"
    );
  }
  return cachedNapi;
}

/** One-shot completion for a {@link ModelSpec} via the Rust gateway (the napi seam). */
export async function invokeModel(
  spec: ModelSpec,
  input: string | ChatMessage[],
  opts?: InvokeOptions
): Promise<ModelResponse> {
  assertKnownProvider(spec.provider);
  const messages = typeof input === "string" ? [{ role: "user", content: input }] : input;
  const request = {
    provider: spec.provider,
    model: spec.model ?? "",
    messages,
    maxTokens: opts?.maxTokens,
    temperature: opts?.temperature
  };
  const napi = loadNapi();
  const responseJson = await napi.llmComplete(JSON.stringify(request), "{}");
  return JSON.parse(responseJson) as ModelResponse;
}

/**
 * The base every per-provider model overlay extends. Carries a {@link ModelSpec}, serializes to
 * it (so `agentNode({ model })` and the napi/pyo3 wire get plain data), and is callable standalone
 * via {@link Model.invoke}. Subclasses set `provider` + sensible defaults + tier helpers.
 */
export abstract class Model {
  /** The serializable declaration this overlay represents. */
  abstract readonly spec: ModelSpec;

  /** The plain {@link ModelSpec} — what the SDK serializes onto the Rust wire. */
  toSpec(): ModelSpec {
    return this.spec;
  }

  /** `JSON.stringify(model)` yields the spec (so a model drops straight into config). */
  toJSON(): ModelSpec {
    return this.spec;
  }

  /** One-shot completion through the Rust gateway. `model.invoke("hi")` works (LangChain-style). */
  invoke(input: string | ChatMessage[], opts?: InvokeOptions): Promise<ModelResponse> {
    return invokeModel(this.spec, input, opts);
  }
}

/** A {@link ModelSpec} | {@link Model} — what `agentNode({ model })` accepts. */
export type ModelLike = ModelSpec | Model;

/** Normalize a {@link ModelLike} to a plain {@link ModelSpec}. */
export function toModelSpec(model: ModelLike): ModelSpec {
  return model instanceof Model ? model.toSpec() : model;
}

/**
 * A generic model over any OpenAI-compatible endpoint (the honest extension point for providers
 * not compiled in by name). Routes through the Rust OpenAI-compatible adapter.
 */
export class OpenAICompatibleModel extends Model {
  readonly spec: ModelSpec;
  constructor(model: string, opts?: { baseURL?: string; apiKeyEnv?: string; tier?: ModelTier }) {
    super();
    this.spec = { provider: "openai", model, baseURL: opts?.baseURL, apiKeyEnv: opts?.apiKeyEnv, tier: opts?.tier };
  }
}

/** `openaiCompatible("model", { baseURL })` — a model on a custom OpenAI-compatible server. */
export function openaiCompatible(
  model: string,
  opts?: { baseURL?: string; apiKeyEnv?: string; tier?: ModelTier }
): OpenAICompatibleModel {
  return new OpenAICompatibleModel(model, opts);
}
