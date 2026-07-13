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
 * This is the seam the control plane (`apps/api`) uses to EXECUTE a graph built from
 * the catalog: it reads each node's metadata, assembles the engine's
 * `EngineSpec.componentNodes` + `agents` maps + the `jsNodeIds` for plain
 * action/tool nodes, and drives the run on the **Rust engine** via `@adriane-ai/napi`.
 *
 * Unlike {@link import("./builder.js").GraphBuilder}, there are no TS handler closures
 * here — components and agents run **natively** in Rust, and plain action/tool nodes
 * are inert JS seams (they return an empty channel update). The carrier IS the wiring.
 *
 * The carrier readers below mirror the canonical Zod schema in
 * `@adriane-ai/contracts` (`node-metadata.ts`); the SDK stays dependency-free of the
 * contracts package, so the narrowing is duplicated structurally here. The control
 * plane is free to validate the carrier with the contracts schema before handing the
 * definition to this runner.
 */

import type { GraphDefinition, GraphState, NodeId, RunId } from "@adriane-ai/graph-core";
import type { RunEvent } from "@adriane-ai/graph-runtime";
import type { ModelTier } from "@adriane-ai/llm-gateway";
// Type-only: keeps the ApprovalEngine contract without pulling its Pg/db implementation
// (and a `pg` dependency) into consumers such as the Studio bundle.
import type { ApprovalEngine } from "@adriane-ai/approval-engine";

import type {
  EfficiencyMiddlewareSpec,
  FsPolicyRule,
  RustAgentConfig,
  RustMapAgentConfig,
  SkillRecord
} from "./agent-node.js";
import { APPROVAL_IDS_CHANNEL, DEFAULT_AGENT_OUTPUT_CHANNEL } from "./agent-node.js";
import type { RustComponentConfig, ComponentKind } from "./components.js";
import {
  rustEngineAvailable,
  tryCreateRustRunner,
  type ApprovedToolWire,
  type RustRunnerParts
} from "./rust-engine.js";
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
  /** ADR 0014 — terse output directive on the system prompt. */
  outputStyle?: "terse";
  /** ADR 0014 — cap (chars) on the agent's seed message (the injected `Input`/`State` dump). */
  contextBudget?: number;
  /** ADR 0022/0023 — durable channel the agent's `writeTodos` list is persisted into. */
  todosChannel?: string;
  /** ADR 0030 phase 9e — channel carrying the run's multimodal input blocks. */
  inputBlocksChannel?: string;
  /** ADR 0026 phase 11 — governed long-term memory overlay. */
  memory?: { namespace: string; topK?: number; recall?: "vector" | "graph" | "both" };
  /** ADR 0035 phase 12 — governed skills (progressive disclosure) overlay. */
  skills?: { namespace: string; required?: string[]; advisoryK?: number };
  /** ADR 0024 — opt this agent into the governed virtual filesystem tools. */
  enableFs?: boolean;
  /**
   * ADR 0025 phase 3d — the resolved efficiency middleware list. Present on graphs built by
   * the phase-3d SDK; absent on a pre-3d persisted node (the Rust bridge then falls back to
   * the legacy `outputStyle`/`contextBudget` knobs above, so old graphs keep their behaviour).
   */
  resolvedMiddleware?: EfficiencyMiddlewareSpec[];
};

/**
 * The mapAgents carrier on `node.metadata.mapAgents` (ADR 0027 phase 4b — dynamic fan-out). Mirrors
 * the contracts schema: run `subAgent` once per item in `overChannel` and collect the per-item results
 * (input order) into `joinAt`. The sub-agent is a full agent carrier → skills/memory/fs/planning apply.
 */
export type MapAgentCarrier = {
  overChannel: string;
  joinAt: string;
  subAgent: AgentCarrier;
  suspendForApproval?: boolean;
};

/** Outcome of a catalog-graph run: the terminal/suspended state and a flat status. */
export type CatalogRunOutcome = {
  /** The final (or suspended) graph state, channels included. */
  state: GraphState;
  /** `"running" | "suspended" | "completed" | "failed"` — the state's status. */
  status: string;
  /** True when execution ran on the Rust engine (always, since this seam requires it). */
  usedRustEngine: true;
  /**
   * Replay-as-evidence (ADR 0038): the recorded LLM I/O + clock journal (`{ decisions, clock }`
   * JSON) when the run executed in record mode (`ADRIANE_LLM_RECORD`); `undefined` otherwise. The
   * control plane persists it to re-feed a later replay (`verify-replay`).
   */
  replayJournal?: string;
  /**
   * Replay-as-evidence (ADR 0040): the run's ENTRY state (initial state, before the entry node ran),
   * surfaced only on a record-mode run; `undefined` otherwise. The control plane persists it as the
   * checkpoint a later `verify-replay` seeds {@link replayCatalogGraph} from.
   */
  entryState?: GraphState;
  /**
   * The pending approvals — the subjects the run requested when it suspended on a gate (empty if it
   * completed without gating). On a {@link replayCatalogGraph} this is what the deterministic
   * re-execution requested: the faithfulness signal `verify-replay` compares to the attested chain.
   */
  pendingApprovals?: { subject: string; reason: string; approvalKey?: string; input?: unknown }[];
};

/** Options for {@link runCatalogGraph} / {@link resumeCatalogGraph}. */
export type RunCatalogGraphOptions = {
  /** A stable run id. Defaults to a generated one. */
  runId?: RunId;
  /** Initial channel data seeding the run. */
  initialData?: Record<string, unknown>;
  /** Subscribe to forwarded run-lifecycle events (every node transition). */
  onEvent?: (event: RunEvent) => void;
  /**
   * Route the run's approvals through an {@link ApprovalEngine}. When present, the
   * agents run natively on Rust as usual, but the moment the run suspends for approval
   * the seam files one request per gated tool (`requestedBy = nodeId`, the agent's own
   * subject) and stashes the engine ids in the `__approvalIds` channel of the returned
   * state — so a human resolves them out of band (the engine forbids self-approval) and
   * the control plane only ever resumes with engine-approved tools. Absent: the run is
   * ungoverned (the legacy channel-only behaviour).
   */
  approvalEngine?: ApprovalEngine;
  /**
   * Per-provider API keys injected by the control plane (ADR 0010), keyed by provider
   * slug (`openai`, `anthropic`, `mistral`, …). Threaded into the Rust `EngineSpec` so
   * the gateway resolves each agent's key tenant-key-first then host env. Omit to rely
   * purely on the host env (local dev, tests).
   */
  providerKeys?: Record<string, string>;
  /**
   * Per-path filesystem permission rules (ADR 0024 phase 2d) the control plane resolved
   * for this run (from its owner-only `fs_path_policy` table), compiled into the engine's
   * `EngineSpec.fsPolicy` and applied to every fs-enabled agent. Omit for fail-closed
   * read-only everywhere.
   */
  fsPolicy?: FsPolicyRule[];
  /**
   * The tenant's governed skills for this run (ADR 0049 B-3) — the control plane's skill store. The
   * engine builds a run-scoped, tenant-isolated store from these and each agent's SkillMiddleware
   * selects from it. Omit/empty → the OSS shared in-memory store (no skills).
   */
  skills?: SkillRecord[];
};

/** Raised when the native engine is unavailable — catalog graphs require it. */
export class RustEngineUnavailableError extends Error {
  public constructor() {
    super(
      "Catalog graphs execute on the Rust engine, but the native addon (@adriane-ai/napi) " +
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

/** Narrow a node's open metadata bag to its mapAgents (dynamic fan-out) carrier, if present + valid. */
export const readMapAgentCarrier = (
  metadata: Record<string, unknown> | undefined
): MapAgentCarrier | undefined => {
  const map = metadata?.mapAgents;
  if (!isRecord(map)) {
    return undefined;
  }
  const { overChannel, joinAt, subAgent } = map;
  if (typeof overChannel !== "string" || overChannel.length === 0) return undefined;
  if (typeof joinAt !== "string" || joinAt.length === 0) return undefined;
  if (!isRecord(subAgent)) return undefined;
  return {
    overChannel,
    joinAt,
    subAgent: subAgent as AgentCarrier,
    suspendForApproval: map.suspendForApproval === true
  };
};

/**
 * Project an {@link AgentCarrier} into the wire {@link RustAgentConfig} the bridge
 * consumes. `usesApprovalEngine` reflects whether the run was given an
 * {@link ApprovalEngine}: on the catalog path the agent still executes natively on Rust
 * (the flag does not re-route it), but the run is governed — the seam files a request
 * per gated tool when the run suspends (see {@link fileApprovalRequests}).
 */
const carrierToAgentConfig = (
  carrier: AgentCarrier,
  usesApprovalEngine: boolean
): RustAgentConfig => ({
  provider: carrier.provider ?? "anthropic",
  model: carrier.model,
  tier: carrier.tier,
  system: carrier.system,
  toolNames: carrier.toolNames ?? [],
  maxIterations: carrier.maxIterations,
  suspendForApproval: carrier.suspendForApproval === true,
  approvalToolNames: carrier.approvalToolNames ?? [],
  outputChannel: carrier.outputChannel ?? DEFAULT_AGENT_OUTPUT_CHANNEL,
  // ADR 0014 token-efficiency knobs + ADR 0022/0023 durable todos channel: carried on
  // the persisted node so the catalog/Studio run path reaches parity with the in-process
  // SDK builder path (toRustAgentConfig), which forwards the same fields.
  outputStyle: carrier.outputStyle,
  contextBudget: carrier.contextBudget,
  todosChannel: carrier.todosChannel,
  inputBlocksChannel: carrier.inputBlocksChannel,
  memory: carrier.memory,
  skills: carrier.skills,
  // ADR 0024 — fs enablement carried on the persisted node; the run's fs policy is
  // supplied separately by the control plane (RunCatalogGraphOptions.fsPolicy).
  enableFs: carrier.enableFs,
  // ADR 0025 phase 3d — forward the resolved efficiency list (already desugared at build
  // time); a pre-3d carrier has none, and the Rust bridge falls back to the flat knobs.
  resolvedMiddleware: carrier.resolvedMiddleware,
  // The catalog path carries no JS tool closures — the agent's tools are native
  // (no-op stubs in the bridge unless a name is also in jsToolNames, which it never is here).
  toolBindings: [],
  usesApprovalEngine
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
const assembleParts = (
  definition: GraphDefinition,
  usesApprovalEngine: boolean,
  providerKeys: Record<string, string> | undefined,
  fsPolicy: FsPolicyRule[] | undefined,
  skills: SkillRecord[] | undefined
): RustRunnerParts<ChannelValues> => {
  const components = new Map<string, RustComponentConfig>();
  const agents = new Map<string, RustAgentConfig>();
  const mapAgents = new Map<string, RustMapAgentConfig>();
  const jsNodeIds = new Set<string>();

  for (const node of definition.nodes) {
    const id = String(node.id);
    const component = readComponentCarrier(node.metadata);
    if (component !== undefined) {
      components.set(id, { kind: component.kind as ComponentKind, params: component.params });
      continue;
    }
    // A mapAgents carrier takes precedence over a plain agent carrier (a fan-out node is not itself a
    // top-level agent) — the bridge routes it via EngineSpec.map_agents, keyed by node id.
    const mapAgent = readMapAgentCarrier(node.metadata);
    if (mapAgent !== undefined) {
      mapAgents.set(id, {
        overChannel: mapAgent.overChannel,
        joinAt: mapAgent.joinAt,
        agent: carrierToAgentConfig(mapAgent.subAgent, usesApprovalEngine),
        suspendForApproval: mapAgent.suspendForApproval === true
      });
      continue;
    }
    // A node that CARRIES a mapAgents key but fails to parse (missing overChannel/joinAt/subAgent) would
    // otherwise fall through to an inert JS node and silently never fan out. Surface it — no silent caps.
    if (isRecord(node.metadata?.mapAgents)) {
      console.warn(
        `[adriane] node "${id}" has a malformed mapAgents carrier (needs overChannel, joinAt, subAgent) — it will NOT fan out.`
      );
    }
    const agent = readAgentCarrier(node.metadata);
    if (agent !== undefined) {
      agents.set(id, carrierToAgentConfig(agent, usesApprovalEngine));
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
    // Catalog graphs (assembled from node-metadata carriers) carry no subgraph nodes.
    subgraphs: [],
    nodeFns: new Map(
      jsNodeIds.size === 0 ? [] : [...jsNodeIds].map((id) => [id, async () => ({})])
    ),
    toolFns: new Map(),
    conditions: new Map(),
    agents,
    components,
    // ADR 0027 phase 4b / ADR 0049 — the catalog path now reads a `mapAgents` carrier (a dynamic
    // fan-out node), at parity with the in-process builder path.
    mapAgents,
    jsNodeIds,
    jsToolNames: new Set(),
    providerKeys,
    fsPolicy,
    skills
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
  const runner = tryCreateRustRunner<ChannelValues>(
    assembleParts(
      definition,
      options.approvalEngine !== undefined,
      options.providerKeys,
      options.fsPolicy,
      options.skills
    )
  );
  if (runner === null) {
    throw new RustEngineUnavailableError();
  }
  if (options.onEvent !== undefined) {
    runner.subscribe(options.onEvent);
  }
  const runId = options.runId ?? generateRunId();
  const state = (await runner.run(runId, options.initialData ?? {})) as unknown as GraphState;
  const governed = await fileApprovalRequests(definition, state, runId, options.approvalEngine);
  return {
    state: governed,
    status: governed.status,
    usedRustEngine: true,
    replayJournal: runner.recordedJournal(),
    entryState: runner.recordedEntryState(),
    pendingApprovals: runner.pendingApprovals()
  };
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
  options: Pick<
    RunCatalogGraphOptions,
    "onEvent" | "approvalEngine" | "providerKeys" | "fsPolicy" | "skills"
  > & {
    /**
     * Human-granted tools to unlock on resume, each carrying its `{ name, requestedBy,
     * resolvedBy }` provenance. Passed straight through to the Rust bridge, which
     * re-validates the no-self-approval invariant per tool on `Entry::Resume` and writes
     * only the validated names into `__approvedTools`. The control plane (`apps/api`)
     * is the authority on which tools were approved (drawn from the ApprovalEngine), but
     * the engine re-checks the provenance here — defence in depth on the PRODUCTION
     * resume path. Omitted/empty: an ordinary resume that unlocks no tools.
     */
    approvedTools?: ApprovedToolWire[];
  } = {}
): Promise<CatalogRunOutcome> => {
  if (!rustEngineAvailable()) {
    throw new RustEngineUnavailableError();
  }
  const runner = tryCreateRustRunner<ChannelValues>(
    assembleParts(
      definition,
      options.approvalEngine !== undefined,
      options.providerKeys,
      options.fsPolicy,
      options.skills
    )
  );
  if (runner === null) {
    throw new RustEngineUnavailableError();
  }
  if (options.onEvent !== undefined) {
    runner.subscribe(options.onEvent);
  }
  const resumed = (await runner.resume(
    state,
    options.approvedTools ?? []
  )) as unknown as GraphState;
  // A resume can itself hit a NEW approval gate; file requests for that suspension too.
  const governed = await fileApprovalRequests(
    definition,
    resumed,
    String(resumed.runId) as RunId,
    options.approvalEngine
  );
  return {
    state: governed,
    status: governed.status,
    usedRustEngine: true,
    replayJournal: runner.recordedJournal()
  };
};

/**
 * Replay-as-evidence (ADR 0038): re-execute a recorded catalog run from `checkpointId`, re-feeding
 * its `replayJournal` (LLM outputs + timestamps from a record-mode {@link runCatalogGraph}) on the
 * Rust engine so the re-derivation is deterministic. A forked, READ-ONLY run — it never files
 * approval requests or opens gates. Returns the replayed state; the caller compares its governed
 * decisions to the attested chain via {@link verifyReplayDecisions}. Requires a native addon with
 * replay support (`engineReplay`); throws otherwise.
 */
export const replayCatalogGraph = async (
  definition: GraphDefinition,
  state: GraphState,
  checkpointId: string,
  replayJournal: string,
  options: Pick<RunCatalogGraphOptions, "onEvent" | "providerKeys" | "fsPolicy" | "skills"> = {}
): Promise<CatalogRunOutcome> => {
  if (!rustEngineAvailable()) {
    throw new RustEngineUnavailableError();
  }
  // No approval engine on replay — it is read-only EVIDENCE and must never open a new gate.
  const runner = tryCreateRustRunner<ChannelValues>(
    assembleParts(definition, false, options.providerKeys, options.fsPolicy, options.skills)
  );
  if (runner === null) {
    throw new RustEngineUnavailableError();
  }
  if (options.onEvent !== undefined) {
    runner.subscribe(options.onEvent);
  }
  const replayed = (await runner.replay(
    state,
    checkpointId,
    replayJournal
  )) as unknown as GraphState;
  // The replay re-suspends at the first gate (no approvals seeded): `pendingApprovals` are the
  // subjects the deterministic re-execution requested — what verify-replay compares to the chain.
  return {
    state: replayed,
    status: replayed.status,
    usedRustEngine: true,
    pendingApprovals: runner.pendingApprovals()
  };
};

/** One approval request the seam files, normalized to the `{ description }` subject. */
type SurfacedApprovalRequest = { subject: { description: string } };

/**
 * Normalize one surfaced `approvalRequests` entry's subject to `{ description }`. The
 * Rust agent emits a FLAT string subject (`"tool:<name>"`, see agents-core
 * `ApprovalRequestItem`); the TS handler emits a `{ description: "tool:<name>" }`
 * object. Accept both, returning `undefined` for anything else.
 */
const normalizeSubject = (request: unknown): SurfacedApprovalRequest | undefined => {
  if (!isRecord(request)) {
    return undefined;
  }
  const subject = (request as { subject?: unknown }).subject;
  if (typeof subject === "string") {
    return { subject: { description: subject } };
  }
  if (isRecord(subject) && typeof (subject as { description?: unknown }).description === "string") {
    return { subject: { description: (subject as { description: string }).description } };
  }
  return undefined;
};

/** Read + normalize an agent output channel's `approvalRequests` off the suspended state. */
const readApprovalRequests = (
  state: GraphState,
  outputChannel: string
): SurfacedApprovalRequest[] => {
  const channel = (state.channels as Record<string, unknown>)[outputChannel];
  if (channel === null || typeof channel !== "object") {
    return [];
  }
  const requests = (channel as { approvalRequests?: unknown }).approvalRequests;
  if (!Array.isArray(requests)) {
    return [];
  }
  return requests
    .map(normalizeSubject)
    .filter((request): request is SurfacedApprovalRequest => request !== undefined);
};

/**
 * File one {@link ApprovalEngine} request per gated tool surfaced by a suspended
 * catalog run, and stash the returned ids in the `__approvalIds` channel of the
 * returned state — mirroring the TS `createAgentNodeHandler` emission pattern
 * (`requestedBy = nodeId`, the agent's own subject). The agent is the requester; a
 * human (a different principal) resolves it out of band, which the engine enforces.
 *
 * No-ops (returns the state unchanged) when no engine is given or the run is not
 * suspended. Idempotency: an agent node that already carries stashed ids (a state that
 * was governed once) is skipped, so re-driving a suspended state does not double-file.
 */
const fileApprovalRequests = async (
  definition: GraphDefinition,
  state: GraphState,
  runId: RunId,
  engine: ApprovalEngine | undefined
): Promise<GraphState> => {
  if (engine === undefined || state.status !== "suspended") {
    return state;
  }
  const channels = { ...(state.channels as Record<string, unknown>) };
  const alreadyStashed = Array.isArray(channels[APPROVAL_IDS_CHANNEL])
    ? (channels[APPROVAL_IDS_CHANNEL] as unknown[]).length > 0
    : false;
  if (alreadyStashed) {
    return state;
  }

  const ids: string[] = [];
  for (const node of definition.nodes) {
    const agent = readAgentCarrier(node.metadata);
    if (agent === undefined) {
      continue;
    }
    const outputChannel = agent.outputChannel ?? DEFAULT_AGENT_OUTPUT_CHANNEL;
    for (const request of readApprovalRequests(state, outputChannel)) {
      const created = await engine.request({
        runId,
        nodeId: String(node.id) as NodeId,
        requestedBy: String(node.id),
        subject: request.subject
      });
      ids.push(String(created.id));
    }
  }

  if (ids.length === 0) {
    return state;
  }
  return { ...state, channels: { ...channels, [APPROVAL_IDS_CHANNEL]: ids } };
};

/** Type guard a node carries either catalog carrier. Useful to decide the run path. */
export const isCatalogGraph = (definition: GraphDefinition): boolean =>
  definition.nodes.some(
    (node) =>
      readComponentCarrier(node.metadata) !== undefined ||
      readAgentCarrier(node.metadata) !== undefined
  );
