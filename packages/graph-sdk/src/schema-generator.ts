import {
  componentCatalog,
  type ComponentCatalogEntry,
  type ComponentParamMeta
} from "./catalog.js";

/**
 * A minimal JSON Schema (the subset we emit). Enough for an AI agent or a validator to know a
 * component's parameter shape before compiling a graph (ADR AI-DX). Generated from the catalog's
 * declared param `type` strings — the single source of truth, so it can't drift from the docs.
 */
export type JsonSchema = {
  type?: string;
  enum?: string[];
  items?: JsonSchema;
  description?: string;
  properties?: Record<string, JsonSchema>;
  required?: string[];
  additionalProperties?: boolean;
};

/** Map a catalog param `type` string (a TS-ish annotation) to a JSON Schema fragment. */
export function paramTypeToJsonSchema(type: string): JsonSchema {
  const t = type.trim();
  // A union of string literals — `"a" | "b" | "c"` → an enum.
  if (t.includes("|") && /^("[^"]*"\s*\|\s*)+"[^"]*"$/.test(t)) {
    const values = t.split("|").map((s) => s.trim().replace(/^"|"$/g, ""));
    return { type: "string", enum: values };
  }
  // Array types — `string[]`, `number[]`, `RouterRule[]`, …
  if (t.endsWith("[]")) {
    const inner = t.slice(0, -2).trim();
    const itemSchema = scalar(inner);
    return itemSchema !== undefined ? { type: "array", items: itemSchema } : { type: "array" };
  }
  return scalar(t) ?? {}; // unknown/complex object type → unconstrained
}

/** Scalar primitives only; returns undefined for anything structural. */
function scalar(t: string): JsonSchema | undefined {
  switch (t) {
    case "string":
      return { type: "string" };
    case "number":
      return { type: "number" };
    case "boolean":
      return { type: "boolean" };
    default:
      return undefined;
  }
}

function paramsToObjectSchema(params: readonly ComponentParamMeta[]): JsonSchema {
  const properties: Record<string, JsonSchema> = {};
  const required: string[] = [];
  for (const p of params) {
    properties[p.name] = { ...paramTypeToJsonSchema(p.type), description: p.description };
    if (p.required) required.push(p.name);
  }
  return {
    type: "object",
    properties,
    ...(required.length > 0 ? { required } : {}),
    additionalProperties: false
  };
}

/** One component's identity + its parameter JSON Schema. */
export type ComponentSchema = {
  kind: string;
  title: string;
  category: string;
  description: string;
  integration: boolean;
  paramsSchema: JsonSchema;
};

/** The JSON Schema of one catalog entry's params. */
export function componentSchema(entry: ComponentCatalogEntry): ComponentSchema {
  return {
    kind: entry.kind,
    title: entry.title,
    category: entry.category,
    description: entry.description,
    integration: entry.integration,
    paramsSchema: paramsToObjectSchema(entry.params)
  };
}

/** Per-node JSON Schemas for the whole component library — what `list_catalog_with_schemas`
 * (MCP) and the docs reference emit. Keyed by `kind`. */
export function componentSchemas(): Record<string, ComponentSchema> {
  const out: Record<string, ComponentSchema> = {};
  for (const entry of componentCatalog) {
    out[entry.kind] = componentSchema(entry);
  }
  return out;
}
