import type { GraphDefinition, GraphState, NodeId, RunId } from "@adriane-ai/graph-core";
import {
  GraphRuntime,
  InMemoryCheckpointer,
  InMemoryConditionRegistry,
  InMemoryEventBus,
  InMemoryNodeRegistry,
  type Checkpointer,
  type ConditionFn,
  type EventBus,
  type NodeExecutionContext,
  type NodeHandler,
  type RunEvent,
  type StreamEvent,
  type StreamMode
} from "@adriane-ai/graph-runtime";

import type { ApprovalId, ApprovalRequest } from "@adriane-ai/approval-engine";

import {
  APPROVED_TOOLS_CHANNEL,
  type AgentApprovalBinding,
  type RustAgentConfig
} from "./agent-node.js";
import type { RustComponentConfig } from "./components.js";
import {
  rustEngineAvailable,
  tryCreateRustRunner,
  type ApprovedToolWire,
  type AsyncNodeFn,
  type AsyncToolFn,
  type RustGraphRunner,
  type RustRunnerParts
} from "./rust-engine.js";
import type { ChannelValues, InitialData, TypedGraphState } from "./typed.js";

/** Options for {@link CompiledGraph.approveAndResume}. */
export type ApproveAndResumeOptions = {
  /** Names of approval-gated tools the human has granted. They execute on resume. */
  approvedTools: string[];
  /**
   * The principal granting the approval — a human, NEVER the agent that requested it.
   * It is recorded as each granted tool's `resolvedBy` and carried to the Rust engine,
   * which rejects the resume if it is empty or equals the tool's requester (the
   * no-self-approval guard-rail). On the TS path, when an agent node was configured with
   * an {@link import("@adriane-ai/approval-engine").ApprovalEngine}, the matching pending
   * requests are approved through the engine under this principal before resuming — so
   * the engine's own `ensureCanResolve` enforces the same invariant. Defaults to
   * `"human"` when omitted.
   */
  resolvedBy?: string;
};

const generateRunId = (): RunId => {
  const random = globalThis.crypto?.randomUUID?.() ?? Math.random().toString(36).slice(2);
  return `run_${random}` as RunId;
};

/** Wiring assembled by {@link GraphBuilder.compile} and handed to a {@link CompiledGraph}. */
export type CompiledGraphParts = {
  definition: GraphDefinition;
  handlers: Map<string, NodeHandler>;
  conditions: Map<string, ConditionFn>;
  /**
   * Per agent node, the serializable config + JS tool executes the Rust engine bridge
   * needs (see {@link RustAgentConfig}). Empty for graphs with no agent nodes.
   */
  agentConfigs?: Map<string, RustAgentConfig>;
  /**
   * Per agent node, the governance binding (the optional {@link ApprovalEngine} plus
   * the principal that requests approvals and the node's gated tool names) that
   * {@link CompiledGraph.approveAndResume} uses to approve pending engine requests on
   * the TS path and to stamp each granted tool's `requestedBy` for the Rust guard-rail.
   * Empty for graphs with no agent nodes.
   */
  agentApprovals?: Map<string, AgentApprovalBinding>;
  /**
   * Per component node, the `{ kind, params }` carrier the Rust engine bridge needs to
   * run the native component handler (see {@link RustComponentConfig}). Empty for
   * graphs with no component nodes.
   */
  componentConfigs?: Map<string, RustComponentConfig>;
};

/** Options accepted by {@link CompiledGraph.run} / {@link CompiledGraph.stream}. */
export type RunOptions = {
  /** Provide a stable run id (e.g. to correlate with an external system). */
  runId?: RunId;
};

/**
 * Engine selection for {@link CompiledGraph}. Read once from `ADRIANE_SDK_ENGINE`:
 * - `"auto"` (default): use the Rust engine for graphs it can run faithfully (agent
 *   nodes, human gates, named conditions), else the TypeScript engine.
 * - `"rust"`: force the Rust engine when the native addon is present.
 * - `"ts"`: force the (deprecated) TypeScript engine.
 * The public SDK API is unchanged — this is an environment escape hatch only.
 */
type EnginePreference = "auto" | "rust" | "ts";

const enginePreference = (): EnginePreference => {
  const raw = process.env.ADRIANE_SDK_ENGINE?.trim().toLowerCase();
  return raw === "rust" || raw === "ts" ? raw : "auto";
};

let warnedTsFallback = false;
const warnTsEngineOnce = (): void => {
  if (warnedTsFallback) {
    return;
  }
  warnedTsFallback = true;
  console.warn(
    "[@adriane-ai/graph-sdk] Executing on the deprecated in-process TypeScript engine. " +
      "Install the native engine addon (npm install @adriane-ai/napi) to run on the Rust engine."
  );
};

/**
 * A validated, runnable graph. Holds the engine wiring (registries, checkpointer,
 * event bus, runtime) so callers don't touch the lower-level `@adriane-ai/graph-runtime`
 * primitives unless they want to.
 *
 * Execution runs on the **Rust engine** via `@adriane-ai/napi` when the native addon is
 * present and the graph is one Rust can run faithfully; otherwise it falls back to
 * the in-process TypeScript {@link GraphRuntime}. The public API is identical either
 * way — `run` / `resume` / `approveAndResume` / `stream` / `onEvent` behave the same.
 */
export class CompiledGraph<TState extends ChannelValues = ChannelValues> {
  public readonly definition: GraphDefinition;
  private readonly checkpointer: Checkpointer;
  private readonly eventBus: EventBus;
  private readonly runtime: GraphRuntime;

  /**
   * The Rust runner, when this graph runs on the Rust engine; else `null` (TS path).
   * Typed on `ChannelValues` (not `TState`) on purpose: the runner round-trips state as
   * serialized `GraphState`/JSON, so it never needs the precise channel shape — and
   * keeping `TState` out of every *field* keeps `CompiledGraph<TState>` variance-friendly
   * (a `CompiledGraph<Specific>` stays assignable to `CompiledGraph<ChannelValues>`,
   * e.g. when stored in a heterogeneous registry). The public methods re-narrow to
   * `TState` at their boundary.
   */
  private readonly rustRunner: RustGraphRunner<ChannelValues> | null;
  /** The last suspended state seen per run id, fed back into the Rust resume/approve. */
  private readonly suspendedStates = new Map<string, GraphState>();
  /** Per agent node, the governance binding used by {@link approveAndResume}. */
  private readonly agentApprovals: Map<string, AgentApprovalBinding>;

  public constructor(parts: CompiledGraphParts) {
    this.definition = parts.definition;
    this.agentApprovals = parts.agentApprovals ?? new Map<string, AgentApprovalBinding>();

    const nodeRegistry = new InMemoryNodeRegistry();
    for (const [nodeId, handler] of parts.handlers) {
      nodeRegistry.register(nodeId as NodeId, handler);
    }

    const conditionRegistry = new InMemoryConditionRegistry();
    for (const [name, fn] of parts.conditions) {
      conditionRegistry.register(name, fn);
    }

    this.checkpointer = new InMemoryCheckpointer();
    this.eventBus = new InMemoryEventBus();

    this.runtime = new GraphRuntime({
      graph: parts.definition,
      nodeRegistry,
      conditionRegistry,
      checkpointer: this.checkpointer,
      eventBus: this.eventBus
    });

    this.rustRunner = this.maybeCreateRustRunner(parts);
    // On the Rust path, mirror forwarded run events into the same event bus the TS
    // path uses, so `onEvent` subscribers see events from either engine identically.
    this.rustRunner?.subscribe((event) => {
      this.eventBus.emit(event);
    });
  }

  /**
   * Decide whether this graph runs on the Rust engine and, if so, build the runner.
   *
   * Since Phase F the napi seam **awaits** a callback's returned `Promise`, so the
   * SDK's genuinely-async JS node handlers and tool `execute` fns round-trip through
   * Rust faithfully — the old "synchronous seam" limitation is gone. Two boundaries
   * remain, and they shape the `"auto"` policy:
   *
   * 1. The Rust agent path builds its **own** LLM gateway from env (Mistral / Anthropic
   *    / Ollama / a deterministic mock); it does *not* use the TS `AgentNodeConfig.llm`.
   *    That is the intended "engine on Rust" behavior (the proven live path). The TS
   *    `llm` is consulted only on the TS fallback path. Observable *structure* (final
   *    status, suspend-on-approval, approve-and-resume, lifecycle events) is identical
   *    across engines on the deterministic mock — proven by the fidelity test in
   *    `rust-engine.test.ts`. Only the `AgentResult.reasoning` *text* differs (the two
   *    mocks emit different strings), which is not part of the structural contract.
   * 2. The TS {@link import("@adriane-ai/approval-engine").ApprovalEngine}-backed approval
   *    flow (file a request per gated tool, read the engine's decision on resume) lives
   *    in `createAgentNodeHandler`; the Rust agent path does not invoke it. So an agent
   *    node configured with `approvalEngine` would not file requests on Rust.
   *
   * Therefore `"auto"` (the default) routes a graph to Rust when the addon is present
   * **unless** any agent node uses a TS `approvalEngine` — that one case stays on the
   * TS engine to preserve the engine-backed approval semantics. Everything else (agent
   * nodes on the Rust gateway, JS action/custom/tool nodes, human gates, named
   * conditions, channel-based `approveAndResume`) runs on Rust. When the addon is
   * absent it falls back to the TS engine. `"rust"` forces Rust regardless (the caller
   * accepts the Rust-gateway / no-`approvalEngine` contract); `"ts"` forces TypeScript.
   * The public SDK API is unchanged across engines.
   *
   * Two narrower limitations are *not* gated on (they affect both `auto` and `rust`,
   * but no SDK API surfaces them as a routing choice): a JS handler that returns a
   * routing {@link import("@adriane-ai/graph-core").Command} (`{ goto }`) has its `goto`
   * dropped on Rust (the seam applies a channel update + static-edge routing — build a
   * conditional edge instead); and a {@link GraphBuilder.toolNode} whose tool is
   * `requiresApproval` *fails* rather than suspends on Rust (its handler throws a
   * `DynamicInterrupt`, which the seam surfaces as a node failure, not a clean
   * suspension). Route such graphs with `ADRIANE_SDK_ENGINE=ts` if you need them.
   */
  private maybeCreateRustRunner(parts: CompiledGraphParts): RustGraphRunner<ChannelValues> | null {
    const preference = enginePreference();
    if (preference === "ts" || !rustEngineAvailable()) {
      return null;
    }

    const agentConfigs = parts.agentConfigs ?? new Map<string, RustAgentConfig>();
    const componentConfigs = parts.componentConfigs ?? new Map<string, RustComponentConfig>();

    // Under `auto`, the one case that genuinely diverges on Rust is a TS-`approvalEngine`
    // agent node (the engine-backed approval flow is TS-only). Keep such graphs on TS.
    // `rust` overrides this (the caller opted in explicitly).
    if (preference === "auto") {
      const usesApprovalEngine = [...agentConfigs.values()].some((config) => config.usesApprovalEngine);
      if (usesApprovalEngine) {
        return null;
      }
    }

    const jsHandlerNodeIds = new Set(parts.handlers.keys());
    // Agent-node handlers are also registered in `parts.handlers` (so the TS path
    // works) — but on the Rust path the agent runs natively, so they are NOT JS node
    // ids. Any *other* handler is a JS action/custom/tool node.
    for (const agentNodeId of agentConfigs.keys()) {
      jsHandlerNodeIds.delete(agentNodeId);
    }
    // Component nodes likewise carry a TS-equivalent handler (the fallback path) but
    // run the NATIVE Rust component handler on the Rust path (the bridge routes a
    // `componentNodes` entry before the JS seam), so they are not JS node ids here.
    for (const componentNodeId of componentConfigs.keys()) {
      jsHandlerNodeIds.delete(componentNodeId);
    }

    // JS tool `execute` fns are async and bridge by default (the seam awaits them).
    const toolFns = this.buildToolFns(agentConfigs);

    const runnerParts: RustRunnerParts<ChannelValues> = {
      definition: parts.definition,
      nodeFns: this.buildNodeFns(jsHandlerNodeIds, parts.handlers),
      toolFns,
      conditions: this.buildConditionFns(parts.conditions),
      agents: agentConfigs,
      components: componentConfigs,
      jsNodeIds: jsHandlerNodeIds,
      jsToolNames: new Set(toolFns.keys())
    };
    return tryCreateRustRunner<ChannelValues>(runnerParts);
  }

  /**
   * Adapt the (async) JS node handlers into the async producers the Rust seam needs.
   * The Rust side awaits the returned promise, so a handler doing real async work
   * round-trips faithfully. A handler that returns a routing {@link Command} (not a
   * plain channel update) is coerced to an empty update — the Rust seam applies a
   * channel-update map only; in-handler routing commands stay a TS-engine feature.
   */
  private buildNodeFns(
    jsNodeIds: Set<string>,
    handlers: Map<string, NodeHandler>
  ): Map<string, AsyncNodeFn<ChannelValues>> {
    const out = new Map<string, AsyncNodeFn<ChannelValues>>();
    for (const nodeId of jsNodeIds) {
      const handler = handlers.get(nodeId);
      if (handler === undefined) {
        continue;
      }
      out.set(nodeId, async (state) => {
        // Match the TS runtime's call convention: the first arg is the channel map
        // (`handler(state.channels, state, ctx)` in graph-runtime). Some handlers —
        // notably the tool node from `createToolNode` — read their channels from this
        // `input` arg, not `state.channels`, so passing `null` would break them.
        const result = await handler(state.channels, state as unknown as GraphState, syntheticContext());
        return toUpdateObject(result);
      });
    }
    return out;
  }

  /** Async tool executes for every JS-backed tool across all agent nodes. */
  private buildToolFns(agentConfigs: Map<string, RustAgentConfig>): Map<string, AsyncToolFn> {
    const out = new Map<string, AsyncToolFn>();
    for (const config of agentConfigs.values()) {
      for (const binding of config.toolBindings) {
        out.set(binding.name, (input) => binding.execute(input));
      }
    }
    return out;
  }

  /** The named condition predicates, retyped for the Rust seam (already synchronous). */
  private buildConditionFns(
    conditions: Map<string, ConditionFn>
  ): Map<string, (state: TypedGraphState<ChannelValues>) => boolean> {
    const out = new Map<string, (state: TypedGraphState<ChannelValues>) => boolean>();
    for (const [name, fn] of conditions) {
      out.set(name, (state) => fn(state as unknown as GraphState));
    }
    return out;
  }

  /** True when this graph executes on the Rust engine. */
  public get usesRustEngine(): boolean {
    return this.rustRunner !== null;
  }

  /** Start a fresh run from the entry node and execute until completion or suspension. */
  public async run(
    initialData: InitialData<TState> = {} as InitialData<TState>,
    options?: RunOptions
  ): Promise<TypedGraphState<TState>> {
    const runId = options?.runId ?? generateRunId();
    if (this.rustRunner !== null) {
      const state = await this.rustRunner.run(runId, initialData as Record<string, unknown>);
      this.captureSuspension(state);
      return state as unknown as TypedGraphState<TState>;
    }
    warnTsEngineOnce();
    const state = await this.runtime.start(runId, initialData);
    return state as TypedGraphState<TState>;
  }

  /** Resume a previously suspended run from its latest checkpoint. */
  public async resume(runId: RunId): Promise<TypedGraphState<TState>> {
    if (this.rustRunner !== null) {
      const suspended = this.requireSuspendedState(runId);
      const state = await this.rustRunner.resume(suspended);
      this.captureSuspension(state);
      return state as unknown as TypedGraphState<TState>;
    }
    warnTsEngineOnce();
    const state = await this.runtime.resume(runId);
    return state as TypedGraphState<TState>;
  }

  /**
   * Grant approval for the named tools and resume a run that suspended for approval
   * (an agent node with `suspendForApproval`). On the Rust path the approved tools are
   * written into `__approvedTools` by the engine before resuming; on the TS path the
   * channel is updated directly. Either way the agent re-runs and executes the
   * now-approved tools instead of gating them again. An agent never approves its own
   * tools; this is the human seam.
   */
  public async approveAndResume(
    runId: RunId,
    options: ApproveAndResumeOptions
  ): Promise<TypedGraphState<TState>> {
    const resolvedBy = options.resolvedBy ?? "human";
    if (this.rustRunner !== null) {
      const suspended = this.requireSuspendedState(runId);
      const wire = this.toApprovedToolWire(options.approvedTools, resolvedBy);
      const state = await this.rustRunner.approveAndResume(suspended, wire);
      this.captureSuspension(state);
      return state as unknown as TypedGraphState<TState>;
    }
    warnTsEngineOnce();
    // Mirror the control-plane authority on the TS path: when an agent node routes
    // approvals through an ApprovalEngine, the granted tools' pending requests are
    // resolved THROUGH the engine (under the distinct `resolvedBy` principal) before
    // resuming — so the engine's own `ensureCanResolve` enforces no-self-approval. The
    // `__approvedTools` channel is written too, covering the no-engine (channel-only)
    // case and the engine case identically.
    await this.approvePendingThroughEngines(runId, options.approvedTools, resolvedBy);
    // Sorted + de-duplicated so the channel write is deterministic regardless of the
    // caller's ordering — matching the Rust guard-rail and the control-plane writer.
    const names = [...new Set(options.approvedTools)].sort();
    await this.runtime.updateState(runId, { [APPROVED_TOOLS_CHANNEL]: names });
    return this.resume(runId);
  }

  /**
   * Project granted tool names into the wire shape the Rust engine validates: each
   * tool carries the principal that requested it (the owning agent node) and the
   * distinct principal granting it. Names are sorted so the wire payload is
   * deterministic regardless of the caller's ordering.
   */
  private toApprovedToolWire(approvedTools: string[], resolvedBy: string): ApprovedToolWire[] {
    return [...approvedTools]
      .sort()
      .map((name) => ({ name, requestedBy: this.requesterOfTool(name), resolvedBy }));
  }

  /** The agent node that declared `toolName` as approval-gated, as the request principal. */
  private requesterOfTool(toolName: string): string {
    for (const binding of this.agentApprovals.values()) {
      if (binding.approvalToolNames.includes(toolName)) {
        return binding.requestedBy;
      }
    }
    // No owning agent on record (e.g. a channel-only grant): fall back to the first
    // agent's requester, else a neutral principal. The Rust guard-rail still rejects a
    // resolver equal to whatever requester we report, so the invariant is preserved.
    const first = this.agentApprovals.values().next().value as AgentApprovalBinding | undefined;
    return first?.requestedBy ?? "agent";
  }

  /**
   * For each agent node with an {@link ApprovalEngine}, approve the pending requests
   * whose gated tool is in `approvedTools`, through the engine, under `resolvedBy`. The
   * engine rejects a self-approval, so this is the TS-side enforcement point that
   * mirrors the Rust guard-rail.
   */
  private async approvePendingThroughEngines(
    runId: RunId,
    approvedTools: string[],
    resolvedBy: string
  ): Promise<void> {
    const granted = new Set(approvedTools);
    for (const binding of this.agentApprovals.values()) {
      const engine = binding.approvalEngine;
      if (engine === undefined) {
        continue;
      }
      const pending = await engine.getPending(runId);
      for (const request of pending) {
        const toolName = toolNameOfSubject(request);
        if (toolName !== undefined && granted.has(toolName)) {
          await engine.approve(request.id as ApprovalId, resolvedBy);
        }
      }
    }
  }

  /**
   * Stream events as the graph executes. See {@link StreamMode} for the available
   * shapes. The Rust engine has no incremental stream surface yet, so when running on
   * Rust this drives a full run and yields a single terminal `state_value`. On the TS
   * engine it streams natively.
   */
  public stream(
    initialData: InitialData<TState>,
    mode: StreamMode,
    options?: RunOptions
  ): AsyncIterable<StreamEvent> {
    const runId = options?.runId ?? generateRunId();
    if (this.rustRunner !== null) {
      return this.streamViaRust(runId, initialData);
    }
    warnTsEngineOnce();
    return this.runtime.stream(runId, initialData, mode);
  }

  /** Single-shot stream for the Rust path: run to terminal state, emit it once. */
  private async *streamViaRust(
    runId: RunId,
    initialData: InitialData<TState>
  ): AsyncIterable<StreamEvent> {
    const state = await this.rustRunner!.run(runId, initialData as Record<string, unknown>);
    this.captureSuspension(state);
    yield { type: "state_value", state: state as unknown as GraphState };
  }

  /** Subscribe to the run-event lifecycle stream. Returns an unsubscribe function. */
  public onEvent(handler: (event: RunEvent) => void): () => void {
    return this.eventBus.subscribe(handler);
  }

  /**
   * Escape hatch for the TS engine: the underlying runtime (time-travel, manual node
   * execution). On the Rust path the runtime is present but **not** the executor; use
   * {@link CompiledGraph.usesRustEngine} to branch, and the run-handle methods
   * (`run` / `resume` / `approveAndResume`) which behave identically across engines.
   */
  public get engine(): GraphRuntime {
    return this.runtime;
  }

  /** Record a run's state if it suspended, so resume/approve can feed it back to Rust. */
  private captureSuspension(state: TypedGraphState<ChannelValues>): void {
    if (state.status === "suspended") {
      this.suspendedStates.set(String(state.runId), state as unknown as GraphState);
    } else {
      this.suspendedStates.delete(String(state.runId));
    }
  }

  private requireSuspendedState(runId: RunId): GraphState {
    const state = this.suspendedStates.get(String(runId));
    if (state === undefined) {
      throw new Error(
        `No suspended state for run '${String(runId)}'. On the Rust engine, resume/approve must ` +
          "follow a suspended run on the same CompiledGraph instance."
      );
    }
    return state;
  }
}

const TOOL_SUBJECT_PREFIX = "tool:";

/**
 * Pull the tool name back out of an approval request's subject. The gated-tool subject
 * is `{ description: "tool:<name>" }` (see `agent-node.ts`); anything else yields
 * `undefined` (the request is not a tool gate we can match by name).
 */
const toolNameOfSubject = (request: ApprovalRequest): string | undefined => {
  const description = (request.subject as { description?: unknown }).description;
  return typeof description === "string" && description.startsWith(TOOL_SUBJECT_PREFIX)
    ? description.slice(TOOL_SUBJECT_PREFIX.length)
    : undefined;
};

/**
 * A minimal {@link NodeExecutionContext} for the Rust seam. The channels-only state
 * the seam delivers carries no memory store, and SDK node handlers reached on the
 * Rust path read `state.channels` only; this satisfies the handler signature without
 * pulling a real store across the boundary.
 */
const syntheticContext = (): NodeExecutionContext =>
  ({ memory: undefined as unknown as NodeExecutionContext["memory"] }) satisfies NodeExecutionContext;

/**
 * Coerce a resolved node-handler result into the channel-update map the Rust seam
 * applies. A plain object is the update directly. A routing {@link Command}
 * (`{ goto, update? }`) contributes only its `update` map — the Rust seam applies a
 * channel-update map and routes by the graph's static edges, so an in-handler `goto`
 * is *not* honored on the Rust path (dynamic in-handler routing stays a TS-engine
 * feature; build conditional edges instead). `null` / primitives yield an empty
 * update, matching Rust's tolerant `parse_update`.
 */
const toUpdateObject = (value: unknown): Record<string, unknown> => {
  if (value === null || typeof value !== "object") {
    return {};
  }
  const maybeCommand = value as { goto?: unknown; update?: unknown };
  if (maybeCommand.goto !== undefined) {
    return maybeCommand.update !== null && typeof maybeCommand.update === "object"
      ? (maybeCommand.update as Record<string, unknown>)
      : {};
  }
  return value as Record<string, unknown>;
};
