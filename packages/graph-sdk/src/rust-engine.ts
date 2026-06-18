import { createRequire } from "node:module";

import type { GraphDefinition, GraphState, NodeId, RunId } from "@adriane-ai/graph-core";
import type { RunEvent } from "@adriane-ai/graph-runtime";

import type { ModelTier } from "@adriane-ai/llm-gateway";

import type { RustAgentConfig } from "./agent-node.js";
import type { RustComponentConfig } from "./components.js";
import type { ChannelValues, TypedGraphState } from "./typed.js";

/**
 * Optional bridge to the Rust engine's async run/resume/approve entry points
 * (`@adriane-ai/napi`). Mirrors {@link import("./rust-validator.js").tryRustValidate}:
 * when the native addon is present, graph **execution** can run on the Rust engine
 * (via `engine_run`/`engine_resume`/`engine_approve_and_resume`), with the SDK's TS
 * condition predicates and node/tool seams called back from Rust over a
 * ThreadsafeFunction. The Rust engine is the required production runtime; when the
 * addon is absent (development, tests, or an uncovered platform) this loader returns
 * `null` and {@link import("./compiled-graph.js").CompiledGraph} uses the in-process
 * TypeScript runtime instead.
 *
 * Boundary contract (napi 2.16, Phase F): the Rust seam now **awaits** the JS
 * callback's returned `Promise` — `call_async::<Promise<T>>(..).await?.await?` drives
 * a returned thenable to its resolved value. So `on_node` returns a `Promise<string>`
 * (channel-update / tool-result JSON) and `on_condition` returns a `Promise<string>`
 * (a boolean-ish string the Rust side reads with `parse_bool`). This is what lets the
 * SDK's genuinely-async TS node handlers and tool `execute` fns round-trip through
 * Rust without aborting the process. `on_event` stays fire-and-forget.
 */
export type EngineNodeCallback = (payloadJson: string) => Promise<string>;
export type EngineConditionCallback = (payloadJson: string) => Promise<string>;
export type EngineEventCallback = (payloadJson: string) => void;

type NativeEngine = {
  engineRun(
    specJson: string,
    onNode: EngineNodeCallback,
    onCondition: EngineConditionCallback,
    onEvent: EngineEventCallback
  ): Promise<string>;
  engineResume(
    specJson: string,
    onNode: EngineNodeCallback,
    onCondition: EngineConditionCallback,
    onEvent: EngineEventCallback
  ): Promise<string>;
  engineApproveAndResume(
    specJson: string,
    onNode: EngineNodeCallback,
    onCondition: EngineConditionCallback,
    onEvent: EngineEventCallback
  ): Promise<string>;
};

let cachedNative: NativeEngine | null | undefined;

const hasEngineFns = (mod: unknown): mod is NativeEngine =>
  typeof mod === "object" &&
  mod !== null &&
  typeof (mod as NativeEngine).engineRun === "function" &&
  typeof (mod as NativeEngine).engineResume === "function" &&
  typeof (mod as NativeEngine).engineApproveAndResume === "function";

const loadNativeEngine = (): NativeEngine | null => {
  if (cachedNative !== undefined) {
    return cachedNative;
  }
  try {
    const requireFn = createRequire(import.meta.url);
    const mod: unknown = requireFn("@adriane-ai/napi");
    cachedNative = hasEngineFns(mod) ? mod : null;
  } catch {
    cachedNative = null;
  }
  return cachedNative;
};

/** True when the native addon exposes the async run bridge (execution can use Rust). */
export const rustEngineAvailable = (): boolean => loadNativeEngine() !== null;

/**
 * An async node-update producer for the Rust seam: given the (channels-only) typed
 * state, resolve to the channel update. The SDK adapts its async `NodeHandler` into
 * this directly — the Rust seam awaits the returned promise (Phase F), so a handler
 * that does real async work (I/O, an LLM call) round-trips faithfully.
 */
export type AsyncNodeFn<TState extends ChannelValues> = (
  state: TypedGraphState<TState>
) => Promise<Record<string, unknown>>;

/** An async tool `execute` for the Rust seam: input in, tool-result value out. */
export type AsyncToolFn = (input: unknown) => Promise<unknown>;

/**
 * What the SDK passes to {@link RustGraphRunner}: the things the Rust engine cannot
 * reconstruct itself — async node-update producers (for JS node ids), async tool
 * executes (for JS tool names), and condition predicates — plus the serializable
 * agent configs and the set of JS node ids.
 */
export type RustRunnerParts<TState extends ChannelValues> = {
  definition: GraphDefinition;
  /** Async node-update producers, keyed by node id (Rust `on_node`, `kind:"node"`). */
  nodeFns: Map<string, AsyncNodeFn<TState>>;
  /** Async tool executes, keyed by tool name (Rust `on_node`, `kind:"tool"`). */
  toolFns: Map<string, AsyncToolFn>;
  /** Named condition predicates, keyed by condition name (Rust `on_condition`). */
  conditions: Map<string, (state: TypedGraphState<TState>) => boolean>;
  /** Per-agent-node serializable config. */
  agents: Map<string, RustAgentConfig>;
  /**
   * Per-component-node serializable config (Phase C `componentNodes` carrier), keyed
   * by node id. The node runs a native Rust component handler; its id may *also* be in
   * {@link jsNodeIds} (its TS fallback handler) — Rust takes the component path.
   */
  components: Map<string, RustComponentConfig>;
  /** Node ids whose handler is a JS closure (action / custom / tool nodes). */
  jsNodeIds: Set<string>;
  /** Tool names that are backed by a JS `execute` (in {@link toolFns}). */
  jsToolNames: Set<string>;
};

/** The `agents` map serialized for the wire (matches Rust `AgentSpec`, camelCase). */
type AgentSpecWire = {
  provider: string;
  model?: string;
  /** Capability tier; the Rust bridge resolves it via `ModelPolicy` when no `model`. */
  tier?: ModelTier;
  system?: string;
  toolNames: string[];
  maxIterations?: number;
  suspendForApproval: boolean;
  approvalToolNames: string[];
  outputChannel: string;
};

/**
 * One native component node serialized for the wire (matches Rust `ComponentNodeSpec`,
 * camelCase): a component `kind` plus its `params` object. Such a node runs the Rust
 * `adriane_components` handler natively instead of the JS seam.
 */
type ComponentNodeSpecWire = {
  kind: string;
  params: Record<string, unknown>;
};

/**
 * One granted tool on the approve path, with the governance provenance the Rust
 * guard-rail validates (matches Rust `ApprovedTool`, camelCase): the principal who
 * *requested* the approval and the (distinct) principal who *resolved* it. The bridge
 * rejects the resume if `resolvedBy` is empty or equals `requestedBy` (no self-approval).
 */
export type ApprovedToolWire = {
  name: string;
  requestedBy: string;
  resolvedBy: string;
};

/** The `EngineSpec` shape the Rust bridge deserializes (camelCase). */
type EngineSpecWire = {
  graph: GraphDefinition;
  runId?: string;
  initialData?: Record<string, unknown>;
  state?: GraphState;
  approvedTools?: ApprovedToolWire[];
  agents: Record<string, AgentSpecWire>;
  /**
   * Per-node native component configuration, keyed by node id. The Phase C carrier:
   * such a node runs a Rust component handler (built at assemble time from `kind` +
   * `params`) and takes precedence over the JS seam even when its id also appears in
   * {@link EngineSpecWire.jsNodeIds}.
   */
  componentNodes: Record<string, ComponentNodeSpecWire>;
  jsNodeIds: string[];
  jsToolNames: string[];
};

/** The `RunOutcome` shape the Rust bridge serializes back. */
type RunOutcomeWire = {
  state: GraphState;
  status: string;
  pendingApprovals: { subject: string; reason: string }[];
};

/** Payload the Rust `on_node` seam sends for a JS node handler or a JS tool. */
type NodePayload =
  | { kind: "node"; nodeId: string; input: unknown; state: Record<string, unknown> }
  | { kind: "tool"; name: string; input: unknown };

/** Payload the Rust `on_condition` seam sends for a named predicate. */
type ConditionPayload = { name: string; state: Record<string, unknown> };

/**
 * Drives a graph on the Rust engine through the napi bridge. One instance per
 * {@link import("./compiled-graph.js").CompiledGraph}; safe to call repeatedly.
 *
 * The TS state the Rust seam hands back is **channels-only** (`{ kind, state }` where
 * `state` is the channel map). We reconstruct a {@link TypedGraphState} carrying those
 * channels so TS conditions and node fns see the same `state.channels` they would on
 * the TS engine — which is what preserves the SDK's typed-state semantics across the
 * boundary. The `GraphState` fields the seam doesn't carry are filled with inert
 * placeholders (SDK conditions and the SDK's own node logic read `state.channels`).
 */
export class RustGraphRunner<TState extends ChannelValues> {
  private readonly native: NativeEngine;
  private readonly parts: RustRunnerParts<TState>;
  private readonly eventSubscribers = new Set<(event: RunEvent) => void>();

  /** Construct only after {@link rustEngineAvailable} returned true. */
  public constructor(native: NativeEngine, parts: RustRunnerParts<TState>) {
    this.native = native;
    this.parts = parts;
  }

  /** Subscribe to forwarded run-lifecycle events. Returns an unsubscribe fn. */
  public subscribe(handler: (event: RunEvent) => void): () => void {
    this.eventSubscribers.add(handler);
    return () => {
      this.eventSubscribers.delete(handler);
    };
  }

  /** Lift the channel map the Rust seam sends into a typed (channels-only) state. */
  private liftState(channels: Record<string, unknown>): TypedGraphState<TState> {
    return {
      runId: "" as RunId,
      graphId: this.parts.definition.id,
      currentNodeId: "" as NodeId,
      status: "running",
      channels: channels as TState,
      version: 0,
      createdAt: "",
      updatedAt: ""
    };
  }

  /**
   * The `on_node` seam: dispatch to an async node fn or tool execute and resolve to
   * its JSON. The Rust side awaits this promise (Phase F), so a handler doing real
   * async work round-trips faithfully. A missing fn resolves to `"{}"` (an empty
   * update) rather than rejecting, matching the Rust side's tolerant `parse_update`.
   */
  private readonly onNode: EngineNodeCallback = async (payloadJson) => {
    const payload = JSON.parse(payloadJson) as NodePayload;
    if (payload.kind === "node") {
      const fn = this.parts.nodeFns.get(payload.nodeId);
      if (fn === undefined) {
        return "{}";
      }
      return JSON.stringify(await fn(this.liftState(payload.state)));
    }
    const tool = this.parts.toolFns.get(payload.name);
    if (tool === undefined) {
      return "{}";
    }
    return JSON.stringify(await tool(payload.input));
  };

  /**
   * The `on_condition` seam: evaluate a named TS predicate against the channels and
   * resolve to its boolean-ish string (`"true"`/`"false"`). The predicate is
   * synchronous (the SDK's {@link TypedCondition} contract), but the Rust side awaits
   * a `Promise<string>`, so we return one. A missing predicate resolves to `"false"`.
   */
  private readonly onCondition: EngineConditionCallback = async (payloadJson) => {
    const payload = JSON.parse(payloadJson) as ConditionPayload;
    const predicate = this.parts.conditions.get(payload.name);
    const value = predicate === undefined ? false : predicate(this.liftState(payload.state));
    return value ? "true" : "false";
  };

  /** The `on_event` seam: forward each run-lifecycle event to subscribers. */
  private readonly onEvent: EngineEventCallback = (payloadJson) => {
    const event = JSON.parse(payloadJson) as RunEvent;
    for (const subscriber of this.eventSubscribers) {
      subscriber(event);
    }
  };

  private buildAgentsWire(): Record<string, AgentSpecWire> {
    const out: Record<string, AgentSpecWire> = {};
    for (const [nodeId, config] of this.parts.agents) {
      out[nodeId] = {
        provider: config.provider,
        model: config.model,
        tier: config.tier,
        system: config.system,
        toolNames: config.toolNames,
        maxIterations: config.maxIterations,
        suspendForApproval: config.suspendForApproval,
        approvalToolNames: config.approvalToolNames,
        outputChannel: config.outputChannel
      };
    }
    return out;
  }

  private buildComponentsWire(): Record<string, ComponentNodeSpecWire> {
    const out: Record<string, ComponentNodeSpecWire> = {};
    for (const [nodeId, config] of this.parts.components) {
      out[nodeId] = { kind: config.kind, params: config.params };
    }
    return out;
  }

  private outcomeToState(outcomeJson: string): TypedGraphState<TState> {
    const outcome = JSON.parse(outcomeJson) as RunOutcomeWire;
    // The wire state is already a valid camelCase GraphState; channels are the typed
    // shape declared by the builder, so this cast is exact (no field reshaping).
    return outcome.state as unknown as TypedGraphState<TState>;
  }

  private baseSpec(): Pick<
    EngineSpecWire,
    "graph" | "agents" | "componentNodes" | "jsNodeIds" | "jsToolNames"
  > {
    return {
      graph: this.parts.definition,
      agents: this.buildAgentsWire(),
      componentNodes: this.buildComponentsWire(),
      jsNodeIds: [...this.parts.jsNodeIds],
      jsToolNames: [...this.parts.jsToolNames]
    };
  }

  /** Start a fresh run on the Rust engine. */
  public async run(
    runId: RunId,
    initialData: Record<string, unknown>
  ): Promise<TypedGraphState<TState>> {
    const spec: EngineSpecWire = { ...this.baseSpec(), runId, initialData };
    const outcomeJson = await this.native.engineRun(
      JSON.stringify(spec),
      this.onNode,
      this.onCondition,
      this.onEvent
    );
    return this.outcomeToState(outcomeJson);
  }

  /**
   * Resume a suspended run from its serialized state. Optionally carries the
   * human-granted `approvedTools` WITH their `{ name, requestedBy, resolvedBy }`
   * provenance (the production catalog resume path): the Rust bridge re-validates the
   * no-self-approval invariant per tool on `Entry::Resume` and writes only the
   * validated names into `__approvedTools` — so a self-approved (or unresolved) tool
   * aborts the resume here too, not just on the approve path. Omitted (or empty) for
   * an ordinary resume past a non-approval gate, which unlocks no tools.
   */
  public async resume(
    state: GraphState,
    approvedTools: ApprovedToolWire[] = []
  ): Promise<TypedGraphState<TState>> {
    const spec: EngineSpecWire = { ...this.baseSpec(), state, approvedTools };
    const outcomeJson = await this.native.engineResume(
      JSON.stringify(spec),
      this.onNode,
      this.onCondition,
      this.onEvent
    );
    return this.outcomeToState(outcomeJson);
  }

  /**
   * Grant the approved tools (each carrying its `{ name, requestedBy, resolvedBy }`
   * provenance), then resume. The Rust bridge re-validates the no-self-approval
   * invariant per tool and writes only the validated names into `__approvedTools`
   * before resuming — a self-approved (or unresolved) tool aborts the resume.
   */
  public async approveAndResume(
    state: GraphState,
    approvedTools: ApprovedToolWire[]
  ): Promise<TypedGraphState<TState>> {
    const spec: EngineSpecWire = { ...this.baseSpec(), state, approvedTools };
    const outcomeJson = await this.native.engineApproveAndResume(
      JSON.stringify(spec),
      this.onNode,
      this.onCondition,
      this.onEvent
    );
    return this.outcomeToState(outcomeJson);
  }
}

/** Build a {@link RustGraphRunner} if the native engine is present, else `null`. */
export const tryCreateRustRunner = <TState extends ChannelValues>(
  parts: RustRunnerParts<TState>
): RustGraphRunner<TState> | null => {
  const native = loadNativeEngine();
  return native === null ? null : new RustGraphRunner<TState>(native, parts);
};
