import { z } from "zod";

import { ModelTierDtoSchema } from "./catalog.js";

/**
 * The SHARED CARRIER for catalog-backed graph nodes.
 *
 * A graph node that is a catalog COMPONENT carries `node.metadata.component =
 * { kind, params }`; a catalog AGENT node carries `node.metadata.agent =
 * { provider?, model?, tier?, system?, toolNames?, maxIterations?,
 *   suspendForApproval?, approvalToolNames?, outputChannel?, outputStyle?,
 *   contextBudget?, todosChannel?, enableFs?, resolvedMiddleware? }`.
 *
 * The graph editor EMITS these into `node.metadata`; the API run path READS
 * `node.metadata` to assemble `EngineSpec.componentNodes` (kind + params) and
 * `EngineSpec.agents` (the agent spec). Typing the carrier here keeps it
 * end-to-end typed across the API ↔ Studio boundary even though the underlying
 * `NodeDefinitionDto.metadata` stays an open `Record<string, unknown>`.
 */

/** A catalog COMPONENT carrier: a `kind` plus its parameter bag. */
export const ComponentNodeMetadataSchema = z.object({
  /** One of `ComponentRegistry::kinds()` (e.g. "promptBuilder") or an integration name. */
  kind: z.string().min(1),
  /** The component's parameter bag; validated by the native factory at assemble time. */
  params: z.record(z.string(), z.unknown())
});

/** A catalog AGENT carrier: mirrors the engine `AgentSpec` (all fields optional in the editor). */
export const AgentNodeMetadataSchema = z.object({
  provider: z.string().min(1).optional(),
  model: z.string().min(1).optional(),
  tier: ModelTierDtoSchema.optional(),
  system: z.string().optional(),
  toolNames: z.array(z.string().min(1)).optional(),
  maxIterations: z.number().int().min(1).optional(),
  suspendForApproval: z.boolean().optional(),
  approvalToolNames: z.array(z.string().min(1)).optional(),
  outputChannel: z.string().min(1).optional(),
  /** ADR 0014 — terse output directive on the system prompt. */
  outputStyle: z.literal("terse").optional(),
  /** ADR 0014 — cap (chars) on the agent's seed message (the injected `Input`/`State` dump). */
  contextBudget: z.number().int().min(1).optional(),
  /** ADR 0022/0023 — durable channel the agent's `writeTodos` list is persisted into. */
  todosChannel: z.string().min(1).optional(),
  /** ADR 0024 phase 2c/2d — opt this agent into the governed virtual filesystem tools. */
  enableFs: z.boolean().optional(),
  /**
   * ADR 0025 phase 3d — the resolved EFFICIENCY middleware list. A discriminated union of
   * efficiency-only kinds (compress / terse / contextBudget): a GOVERNANCE kind (redact /
   * approvalGate / fsPolicy) fails this schema BY CONSTRUCTION, so any consumer that runs it
   * (e.g. `readAgentMetadata`) drops the malformed agent. This is a **type-level + validation
   * guarantee** for callers that opt to validate; it is NOT auto-applied on the persisted
   * catalog run path today (that path reads an unvalidated carrier — the RUNTIME enforcer
   * there is the Rust bridge, whose match only honours efficiency kinds and ignores governance
   * kinds). The SDK's `toRustAgentConfig` throw-gate covers the in-process builder path.
   */
  resolvedMiddleware: z
    .array(
      z.discriminatedUnion("kind", [
        z.object({ kind: z.literal("compress") }),
        z.object({ kind: z.literal("terse") }),
        z.object({ kind: z.literal("contextBudget"), params: z.object({ chars: z.number().int().min(1) }) })
      ])
    )
    .optional()
});

/**
 * The slice of `node.metadata` the catalog carrier owns. Both keys are optional;
 * a node carries at most one (a component OR an agent). Other metadata keys are
 * untouched.
 */
export const CatalogNodeMetadataSchema = z
  .object({
    component: ComponentNodeMetadataSchema.optional(),
    agent: AgentNodeMetadataSchema.optional()
  })
  .passthrough();

export type ComponentNodeMetadata = z.infer<typeof ComponentNodeMetadataSchema>;
export type AgentNodeMetadata = z.infer<typeof AgentNodeMetadataSchema>;
export type CatalogNodeMetadata = z.infer<typeof CatalogNodeMetadataSchema>;

/** Narrow an open metadata bag to its component carrier, if present and valid. */
export const readComponentMetadata = (
  metadata: Record<string, unknown> | undefined
): ComponentNodeMetadata | undefined => {
  if (metadata === undefined) {
    return undefined;
  }
  const parsed = ComponentNodeMetadataSchema.safeParse(metadata.component);
  return parsed.success ? parsed.data : undefined;
};

/** Narrow an open metadata bag to its agent carrier, if present and valid. */
export const readAgentMetadata = (
  metadata: Record<string, unknown> | undefined
): AgentNodeMetadata | undefined => {
  if (metadata === undefined) {
    return undefined;
  }
  const parsed = AgentNodeMetadataSchema.safeParse(metadata.agent);
  return parsed.success ? parsed.data : undefined;
};
