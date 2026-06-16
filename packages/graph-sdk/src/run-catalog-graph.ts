/**
 * Run a **catalog graph** on the Rust engine.
 *
 * A catalog graph is a plain {@link GraphDefinition} (e.g. one authored in the Studio
 * graph editor, persisted as data, with no in-process TS handlers) whose nodes carry
 * the SHARED CARRIER in `node.metadata`:
 *
 *   - a COMPONENT node carries `node.metadata.component = { kind, params }`
 *   - an AGENT node carries `node.metadata.agent = { provider?, model?, tier?, system?,
 *     toolNames?, maxIterations?, suspendForApproval?, approvalToolNames?, outputChannel? }`
 *
 * This is the seam a control plane uses to EXECUTE a graph built from
 * the catalog: it reads each node's metadata, assembles the engine's
 * `EngineSpec.componentNodes` + `agents` maps + the `jsNodeIds` for plain
 * action/tool nodes, and drives the run on the **Rust engine** via `@adriane/napi`.
 *
 * Unlike {@link import("./builder.js").GraphBuilder}, there are no TS handler closures
 * here — components and agents run **natively** in Rust, and plain action/tool nodes
 * are inert JS seams (they return an empty channel update). The carrier IS the wiring.
 *
 * The carrier readers below mirror the canonical Zod schema in
 * `@adriane/contracts` (`node-metadata.ts`); the SDK stays dependency-free of the
 * contracts package, so the narrowing is duplicated structurally here. The control
 * plane is free to validate the carrier with the contracts schema before handing the
 * definition to this runner.
 */

import type { GraphDefinition, GraphState, RunId } from "@adriane/graph-core";
import type { RunEvent } from "@adriane/graph-runtime";
import type { ModelTier } from "@adriane/llm-gateway";

import type { RustAgentConfig } from "./agent-node.js";
import { DEFAULT_AGENT_OUTPUT_CHANNEL } from "./agent-node.js";
import type { RustComponentConfig, ComponentKind } from "./components.js";
import { rustEngineAvailable, tryCreateRustRunner, type RustRunnerParts } from "./rust-engine.js";
import type { ChannelValues } from "./typed.js";

/** The component carrier on `node.metadata.component`. Mirrors the contracts schema. */
export type ComponentCarrier = {
  kind: string;
  params: Record<string, unknown>;
};

/** The agent carrier on `node.metadata.agent`. Mirrors the contracts schema. */
export type AgentCarrier = {
  provider?: string;
  model?: string;
  tier?: ModelTier;
  system?: string;
  toolNames?: string[];
  maxIterations?: number;
  suspendForApproval?: boolean;
  approvalToolNames?: string[];
  outputChannel?: string;
};

/** Outcome of a catalog-graph run: the terminal/suspended state and a flat status. */
export type CatalogRunOutcome = {
  /** The final (or suspended) graph state, channels included. */
  state: GraphState;
  /** `"running" | "suspended" | "completed" | "failed"` — the state's status. */
  status: string;
  /** True when execution ran on the Rust engine (always, since this seam requires it). */
  usedRustEngine: true;
};

/** Options for {@link runCatalogGraph} / {@link resumeCatalogGraph}. */
export type RunCatalogGraphOptions = {
  /** A stable run id. Defaults to a generated one. */
  runId?: RunId;
  /** Initial channel data seeding the run. */
  initialData?: Record<string, unknown>;
  /** Subscribe to forwarded run-lifecycle events (every node transition). */
  onEvent?: (event: RunEvent) => void;
};

/** Raised when the native engine is unavailable — catalog graphs require it. */
export class RustEngineUnavailableError extends Error {
  public constructor() {
    super(
      "Catalog graphs execute on the Rust engine, but the native addon (@adriane/napi) " +
        "is not available. Build it with scripts/build-napi.sh."
    );
    this.name = "RustEngineUnavailableError";
  }
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** Narrow a node's open metadata bag to its COMPONENT carrier, if present and valid. */
export const readComponentCarrier = (
  metadata: Record<string, unknown> | undefined
): ComponentCarrier | undefined => {
  const component = metadata?.component;
  if (!isRecord(component)) {
    return undefined;
  }
  const { kind, params } = component;
  if (typeof kind !== "string" || kind.length === 0) {
    return undefined;
  }
  return { kind, params: isRecord(params) ? params : {} };
};

/** Narrow a node's open metadata bag to its AGENT carrier, if present and valid. */
export const readAgentCarrier = (
  metadata: Record<string, unknown> | undefined
): AgentCarrier | undefined => {
  const agent = metadata?.agent;
  if (!isRecord(agent)) {
    return undefined;
  }
  return agent as AgentCarrier;
};

/** Project an {@link AgentCarrier} into the wire {@link RustAgentConfig} the bridge consumes. */
const carrierToAgentConfig = (carrier: AgentCarrier): RustAgentConfig => ({
  provider: carrier.provider ?? "anthropic",
  model: carrier.model,
  tier: carrier.tier,
  system: carrier.system,
  toolNames: carrier.toolNames ?? [],
  maxIterations: carrier.maxIterations,
  suspendForApproval: carrier.suspendForApproval === true,
  approvalToolNames: carrier.approvalToolNames ?? [],
  outputChannel: carrier.outputChannel ?? DEFAULT_AGENT_OUTPUT_CHANNEL,
  // The catalog path carries no JS tool closures — the agent's tools are native
  // (no-op stubs in the bridge unless a name is also in jsToolNames, which it never is here).
  toolBindings: [],
  usesApprovalEngine: false
});

const generateRunId = (): RunId => {
  const random = globalThis.crypto?.randomUUID?.() ?? Math.random().toString(36).slice(2);
  return `run_${random}` as RunId;
};

/**
 * Assemble the {@link RustRunnerParts} for a catalog graph from its node-metadata
 * carriers. Component and agent nodes are routed to native Rust handlers; every other
 * non-human-gate node becomes an inert JS node (an empty channel update) so a graph
 * that mixes catalog nodes with plain action/tool nodes still runs end-to-end.
 */
const assembleParts = (definition: GraphDefinition): RustRunnerParts<ChannelValues> => {
  const components = new Map<string, RustComponentConfig>();
  const agents = new Map<string, RustAgentConfig>();
  const jsNodeIds = new Set<string>();

  for (const node of definition.nodes) {
    const id = String(node.id);
    const component = readComponentCarrier(node.metadata);
    if (component !== undefined) {
      components.set(id, { kind: component.kind as ComponentKind, params: component.params });
      continue;
    }
    const agent = readAgentCarrier(node.metadata);
    if (agent !== undefined) {
      agents.set(id, carrierToAgentConfig(agent));
      continue;
    }
    if (node.type === "human-gate") {
      // The runtime suspends natively at a human gate — no handler needed.
      continue;
    }
    // A plain action / tool / custom node with no carrier: an inert JS seam. The
    // catalog path has no TS handler closures, so it produces an empty update.
    jsNodeIds.add(id);
  }

  return {
    definition,
    nodeFns: new Map(jsNodeIds.size === 0 ? [] : [...jsNodeIds].map((id) => [id, async () => ({})])),
    toolFns: new Map(),
    conditions: new Map(),
    agents,
    components,
    jsNodeIds,
    jsToolNames: new Set()
  };
};

/**
 * Run a catalog {@link GraphDefinition} (whose nodes carry `node.metadata.component`
 * and `node.metadata.agent`) to completion or suspension on the **Rust engine**.
 *
 * Throws {@link RustEngineUnavailableError} when the native addon is absent.
 */
export const runCatalogGraph = async (
  definition: GraphDefinition,
  options: RunCatalogGraphOptions = {}
): Promise<CatalogRunOutcome> => {
  if (!rustEngineAvailable()) {
    throw new RustEngineUnavailableError();
  }
  const runner = tryCreateRustRunner<ChannelValues>(assembleParts(definition));
  if (runner === null) {
    throw new RustEngineUnavailableError();
  }
  if (options.onEvent !== undefined) {
    runner.subscribe(options.onEvent);
  }
  const runId = options.runId ?? generateRunId();
  const state = await runner.run(runId, options.initialData ?? {});
  return { state: state as unknown as GraphState, status: state.status, usedRustEngine: true };
};

/**
 * Resume a previously-suspended catalog run (e.g. past a human gate) from its
 * serialized {@link GraphState}, on the **Rust engine**. The bridge seeds its
 * checkpointer with this state and resumes from it.
 *
 * Throws {@link RustEngineUnavailableError} when the native addon is absent.
 */
export const resumeCatalogGraph = async (
  definition: GraphDefinition,
  state: GraphState,
  options: Pick<RunCatalogGraphOptions, "onEvent"> = {}
): Promise<CatalogRunOutcome> => {
  if (!rustEngineAvailable()) {
    throw new RustEngineUnavailableError();
  }
  const runner = tryCreateRustRunner<ChannelValues>(assembleParts(definition));
  if (runner === null) {
    throw new RustEngineUnavailableError();
  }
  if (options.onEvent !== undefined) {
    runner.subscribe(options.onEvent);
  }
  const resumed = await runner.resume(state);
  return {
    state: resumed as unknown as GraphState,
    status: resumed.status,
    usedRustEngine: true
  };
};

/** Type guard a node carries either catalog carrier. Useful to decide the run path. */
export const isCatalogGraph = (definition: GraphDefinition): boolean =>
  definition.nodes.some(
    (node) =>
      readComponentCarrier(node.metadata) !== undefined ||
      readAgentCarrier(node.metadata) !== undefined
  );
