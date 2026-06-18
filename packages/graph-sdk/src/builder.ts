import {
  validateGraph,
  type ChannelDefinition,
  type ChannelReducer,
  type EdgeDefinition,
  type EdgeId,
  type GraphDefinition,
  type GraphId,
  type Message,
  type NodeDefinition,
  type NodeId,
  type NodeType,
  type RetryPolicy
} from "@adriane-ai/graph-core";
import type { AgentResult } from "@adriane-ai/agents-core";
import type { ConditionFn, NodeHandler } from "@adriane-ai/graph-runtime";

import { CompiledGraph } from "./compiled-graph.js";
import {
  APPROVAL_IDS_CHANNEL,
  APPROVED_TOOLS_CHANNEL,
  createAgentNodeHandler,
  createToolNodeHandler,
  DEFAULT_AGENT_OUTPUT_CHANNEL,
  toAgentApprovalBinding,
  toRustAgentConfig,
  type AgentApprovalBinding,
  type AgentNodeConfig,
  type RustAgentConfig,
  type ToolNodeConfig
} from "./agent-node.js";
import type { ComponentDescriptor, RustComponentConfig } from "./components.js";
import { DuplicateNodeError, GraphCompileError, MissingHandlerError, type Result } from "./errors.js";
import { tryRustValidate } from "./rust-validator.js";
import type { ChannelValues, EmptyChannels, TypedCondition, TypedNodeHandler } from "./typed.js";

/** Options passed to {@link createGraph}. */
export type CreateGraphOptions = {
  name: string;
  /** Defaults to a slugified `name`. */
  id?: string;
  /** Semver-ish version string. Defaults to `"0.0.0"`. */
  version?: string;
  recursionLimit?: number;
  metadata?: Record<string, unknown>;
};

/** Channel shorthand: `reducer` defaults to `"replace"`. The value type is inferred from `default`. */
export type ChannelInput<TValue = unknown> = {
  type: string;
  reducer?: ChannelReducer;
  default?: TValue;
};

/** Config form for non-trivial nodes. A bare handler is the common case. */
export type NodeInput<TState extends ChannelValues> = {
  type?: NodeType;
  handler?: TypedNodeHandler<TState>;
  label?: string;
  retryPolicy?: RetryPolicy;
  metadata?: Record<string, unknown>;
};

const slugify = (value: string): string =>
  value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "") || "graph";

/**
 * Fluent builder for an Adriane graph. Add channels, nodes and edges, then
 * {@link GraphBuilder.compile} into a runnable {@link CompiledGraph}.
 *
 * The `TState` type parameter accumulates the declared channels as you call
 * `.channel(...)`, so handler state and the result of `run`/`resume` are fully
 * typed without any manual annotation.
 *
 * Conditions are always **named predicates** registered here — never `eval`'d
 * strings — which is what keeps conditional routing safe and inspectable.
 */
export class GraphBuilder<TState extends ChannelValues = EmptyChannels> {
  private readonly options: CreateGraphOptions;
  private readonly channels: Record<string, ChannelDefinition<unknown>> = {};
  private readonly nodes: NodeDefinition[] = [];
  private readonly edges: EdgeDefinition[] = [];
  private readonly handlers = new Map<string, NodeHandler>();
  private readonly conditions = new Map<string, ConditionFn>();
  /** Per agent node, the serializable config the Rust engine bridge needs. */
  private readonly agentConfigs = new Map<string, RustAgentConfig>();
  /** Per agent node, the governance binding (approval engine + requester) for resume. */
  private readonly agentApprovals = new Map<string, AgentApprovalBinding>();
  /** Per component node, the `{ kind, params }` carrier the Rust engine bridge needs. */
  private readonly componentConfigs = new Map<string, RustComponentConfig>();
  private entryNodeId: string | undefined;

  public constructor(options: CreateGraphOptions) {
    this.options = options;
  }

  /** Reinterpret `this` under a wider channel type after declaring a new channel. */
  private widen<TNext extends ChannelValues>(): GraphBuilder<TNext> {
    return this as unknown as GraphBuilder<TNext>;
  }

  /** Declare a state channel. `reducer` defaults to `"replace"`; value type inferred from `default`. */
  public channel<TName extends string, TValue = unknown>(
    name: TName,
    definition: ChannelInput<TValue>
  ): GraphBuilder<TState & { [K in TName]: TValue }> {
    this.channels[name] = {
      type: definition.type,
      reducer: definition.reducer ?? "replace",
      default: definition.default
    };
    return this.widen<TState & { [K in TName]: TValue }>();
  }

  /** Declare an append-reduced `messages` channel (the conversational default). */
  public messagesChannel<TName extends string = "messages">(
    name: TName = "messages" as TName
  ): GraphBuilder<TState & { [K in TName]: Message[] }> {
    this.channels[name] = { type: "messages", reducer: "append", default: [] };
    return this.widen<TState & { [K in TName]: Message[] }>();
  }

  /** Shared node-registration path: dedupe, register the handler, default the entry. */
  private pushNode(
    id: string,
    type: NodeType,
    label: string,
    handler?: NodeHandler,
    extras?: { retryPolicy?: RetryPolicy; metadata?: Record<string, unknown> }
  ): void {
    if (this.handlers.has(id) || this.nodes.some((node) => String(node.id) === id)) {
      throw new DuplicateNodeError(id);
    }
    if (handler !== undefined) {
      this.handlers.set(id, handler);
    }
    this.nodes.push({
      id: id as NodeId,
      type,
      label,
      retryPolicy: extras?.retryPolicy,
      metadata: extras?.metadata
    });
    this.entryNodeId ??= id;
  }

  /** Declare a channel only if it hasn't been declared yet (for node helpers that need one). */
  private ensureChannel(name: string, definition: ChannelDefinition<unknown>): void {
    if (!(name in this.channels)) {
      this.channels[name] = definition;
    }
  }

  /** Add a node. Pass a handler for the common action case, or a config object. */
  public node(id: string, handlerOrConfig: TypedNodeHandler<TState> | NodeInput<TState>): this {
    const config: NodeInput<TState> =
      typeof handlerOrConfig === "function" ? { type: "action", handler: handlerOrConfig } : handlerOrConfig;

    const type: NodeType = config.type ?? "action";
    if (type === "action" && config.handler === undefined) {
      throw new MissingHandlerError(id);
    }

    this.pushNode(id, type, config.label ?? id, config.handler as NodeHandler | undefined, {
      retryPolicy: config.retryPolicy,
      metadata: config.metadata
    });
    return this;
  }

  /** Convenience for a `human-gate` node that suspends the run for approval. */
  public humanGate(id: string, options?: { label?: string }): this {
    this.pushNode(id, "human-gate", options?.label ?? id);
    return this;
  }

  /**
   * Add an agent node: a ReAct agent driven by an LLM gateway. Its result lands in
   * `config.outputChannel` (default `"agentResult"`), which is auto-declared and
   * added to the typed state. Route on `result.requiresHumanReview` to gate
   * sensitive tool use.
   */
  public agentNode<TOut extends string = typeof DEFAULT_AGENT_OUTPUT_CHANNEL>(
    id: string,
    config: AgentNodeConfig & { outputChannel?: TOut }
  ): GraphBuilder<TState & { [K in TOut]: AgentResult }> {
    const outputChannel = config.outputChannel ?? DEFAULT_AGENT_OUTPUT_CHANNEL;
    // Capture the serializable projection so the Rust engine bridge can run this agent
    // node natively; the TS handler keeps the TS-engine path working.
    const rustConfig = toRustAgentConfig(id, config);
    // Also emit the SHARED CARRIER on `node.metadata.agent` (the wire-serializable
    // fields only — no LLM gateway, no tool closures) so the persisted GraphDefinition
    // is executable by the control plane's catalog run path and renderable in Studio.
    this.pushNode(id, "agent", config.label ?? id, createAgentNodeHandler(id, config), {
      metadata: {
        agent: {
          provider: rustConfig.provider,
          model: rustConfig.model,
          tier: rustConfig.tier,
          system: rustConfig.system,
          toolNames: rustConfig.toolNames,
          maxIterations: rustConfig.maxIterations,
          suspendForApproval: rustConfig.suspendForApproval,
          approvalToolNames: rustConfig.approvalToolNames,
          outputChannel: rustConfig.outputChannel
        }
      }
    });
    this.agentConfigs.set(id, rustConfig);
    this.agentApprovals.set(id, toAgentApprovalBinding(id, config));
    this.ensureChannel(outputChannel, { type: "agentResult", reducer: "replace" });
    // Channels the control plane / ApprovalEngine use to gate and resume.
    this.ensureChannel(APPROVED_TOOLS_CHANNEL, { type: "string[]", reducer: "replace", default: [] });
    this.ensureChannel(APPROVAL_IDS_CHANNEL, { type: "string[]", reducer: "replace", default: [] });
    return this.widen<TState & { [K in TOut]: AgentResult }>();
  }

  /**
   * Add a tool node: executes the tool calls emitted by the last AI message in the
   * `messages` channel (auto-declared as an append-reduced messages channel).
   */
  public toolNode(id: string, config: ToolNodeConfig): GraphBuilder<TState & { messages: Message[] }> {
    this.pushNode(id, "tool", config.label ?? id, createToolNodeHandler(config));
    this.ensureChannel("messages", { type: "messages", reducer: "append", default: [] });
    return this.widen<TState & { messages: Message[] }>();
  }

  /**
   * Add a **component node**: a pure (no-LLM) compute building block from
   * {@link import("./components.js").components} (e.g. `promptBuilder`, `router`,
   * `retriever`). The node carries the Phase C carrier (`{ kind, params }`) so it runs
   * natively on the Rust engine, *and* registers the descriptor's equivalent TS handler
   * so the TS fallback path stays faithful when the native addon is absent.
   *
   * On the Rust path the component takes precedence over the JS seam even though its id
   * is also a JS node id — the bridge routes a `componentNodes` entry to the native
   * handler. So the node always runs the same logic on either engine.
   *
   * ```ts
   * createGraph({ name: "p" })
   *   .channel("name", { type: "string", default: "" })
   *   .channel("prompt", { type: "string", default: "" })
   *   .component("build", components.promptBuilder({ template: "Hi {{name}}", into: "prompt" }));
   * ```
   */
  public component(id: string, descriptor: ComponentDescriptor, options?: { label?: string }): this {
    // Push as an `action` node carrying the TS-equivalent handler (the TS fallback
    // path) AND the SHARED CARRIER on `node.metadata.component` so the persisted
    // GraphDefinition is executable by the control plane's catalog run path
    // (see run-catalog-graph.ts) and renderable in the Studio editor. The Rust path
    // runs the native component handler, keyed by the `componentConfigs` carrier below.
    this.pushNode(id, "action", options?.label ?? id, descriptor.handler, {
      metadata: { component: { kind: descriptor.kind, params: descriptor.params } }
    });
    this.componentConfigs.set(id, { kind: descriptor.kind, params: descriptor.params });
    return this;
  }

  /** Add an unconditional edge from one node to another. */
  public edge(from: string, to: string): this {
    this.edges.push({
      id: `e_${from}_${to}_${this.edges.length}` as EdgeId,
      from: from as NodeId,
      to: to as NodeId,
      type: "default"
    });
    return this;
  }

  /**
   * Add a conditional edge guarded by a named predicate. The predicate is
   * registered under `conditionName` and evaluated against the live (typed) state.
   */
  public conditionalEdge(
    from: string,
    to: string,
    conditionName: string,
    predicate: TypedCondition<TState>
  ): this {
    this.conditions.set(conditionName, predicate as ConditionFn);
    this.edges.push({
      id: `e_${from}_${to}_${this.edges.length}` as EdgeId,
      from: from as NodeId,
      to: to as NodeId,
      type: "conditional",
      condition: conditionName
    });
    return this;
  }

  /** Override the entry node (defaults to the first node added). */
  public entry(nodeId: string): this {
    this.entryNodeId = nodeId;
    return this;
  }

  private buildDefinition(): GraphDefinition {
    return {
      id: (this.options.id ?? slugify(this.options.name)) as GraphId,
      version: this.options.version ?? "0.0.0",
      name: this.options.name,
      recursionLimit: this.options.recursionLimit,
      channels: this.channels,
      nodes: this.nodes,
      edges: this.edges,
      entryNodeId: (this.entryNodeId ?? "") as NodeId,
      metadata: this.options.metadata
    };
  }

  /** Validate and compile, returning a {@link Result} instead of throwing. */
  public safeCompile(): Result<CompiledGraph<TState>, GraphCompileError> {
    const definition = this.buildDefinition();
    // Validate via the Rust core when its native addon is present; otherwise TS.
    const errors = tryRustValidate(definition) ?? validateGraph(definition);
    if (errors.length > 0) {
      return { success: false, error: new GraphCompileError(errors) };
    }

    return {
      success: true,
      data: new CompiledGraph<TState>({
        definition,
        handlers: this.handlers,
        conditions: this.conditions,
        agentConfigs: this.agentConfigs,
        agentApprovals: this.agentApprovals,
        componentConfigs: this.componentConfigs
      })
    };
  }

  /** Validate and compile into a runnable graph. Throws {@link GraphCompileError} on failure. */
  public compile(): CompiledGraph<TState> {
    const result = this.safeCompile();
    if (!result.success) {
      throw result.error;
    }
    return result.data;
  }
}

/** Entry point: start building a graph. */
export const createGraph = (options: CreateGraphOptions): GraphBuilder<EmptyChannels> =>
  new GraphBuilder<EmptyChannels>(options);
