import { ReActAgent, type AgentId, type ToolRegistry } from "@adriane-ai/agents-core";
import {
  InMemoryPromptRegistry,
  ModelPolicy,
  type LLMGateway,
  type LLMProvider,
  type ModelTier,
  type PromptRegistry
} from "@adriane-ai/llm-gateway";
import { createToolNode, DynamicInterrupt, type NodeHandler } from "@adriane-ai/graph-runtime";
// Type-only: keeps the ApprovalEngine contract without pulling its Pg/db implementation
// (and a `pg` dependency) into consumers such as the Studio bundle.
import type { ApprovalEngine, ApprovalId } from "@adriane-ai/approval-engine";
import type { NodeId, RunId } from "@adriane-ai/graph-core";

/** Default channel an agent node writes its {@link import("@adriane-ai/agents-core").AgentResult} into. */
export const DEFAULT_AGENT_OUTPUT_CHANNEL = "agentResult";

/**
 * Channel holding the names of tools whose human approval has been granted. The
 * control plane writes it (see `CompiledGraph.approveAndResume`) before resuming a
 * run that suspended for approval; the agent then executes those tools.
 */
export const APPROVED_TOOLS_CHANNEL = "__approvedTools";

/** Reason carried by the dynamic interrupt an agent node raises when it needs approval. */
export const AGENT_APPROVAL_INTERRUPT = "agent-approval-required";

/**
 * Channel holding the ApprovalEngine request ids created when a run suspends for
 * approval. On resume the node looks each up; the ones the engine reports as
 * `approved` unlock their tools — the engine is the source of truth, not a flag.
 */
export const APPROVAL_IDS_CHANNEL = "__approvalIds";

const TOOL_SUBJECT_PREFIX = "tool:";

/** Where an agent node gets its system prompt. */
export type AgentPromptSource =
  | { registry: PromptRegistry; id: string; version?: string }
  /** Inline convenience: the SDK registers this string and references it by id. */
  | { system: string };

/** Config for {@link GraphBuilder.agentNode}. */
export type AgentNodeConfig = {
  llm: LLMGateway;
  prompt: AgentPromptSource;
  tools?: ToolRegistry;
  provider?: LLMProvider;
  model?: string;
  /**
   * Abstract capability tier (`"frontier" | "balanced" | "fast" | "creative"`). When
   * set and no explicit {@link AgentNodeConfig.model} is given, the concrete model is
   * resolved by the {@link ModelPolicy}: on the Rust path the bridge resolves it from
   * the process env (so "I only have Mistral" maps every tier to the mistral column);
   * on the TS fallback path the SDK resolves it here against `availableFromEnv()` so
   * the agent runs on a consistent concrete provider+model. An explicit `model` (and
   * an explicit `provider`) always wins over the tier (the override stays `false`-
   * recommended in policy terms).
   */
  tier?: ModelTier;
  maxIterations?: number;
  name?: string;
  description?: string;
  /** Channel that receives the agent's result. Defaults to {@link DEFAULT_AGENT_OUTPUT_CHANNEL}. */
  outputChannel?: string;
  /**
   * When true, the node suspends the whole run (a dynamic interrupt) the moment the
   * agent needs approval, instead of just flagging `requiresHumanReview`. Resume with
   * `CompiledGraph.approveAndResume(runId, { approvedTools })` to continue. Default false.
   */
  suspendForApproval?: boolean;
  /**
   * Route approvals through an {@link ApprovalEngine}: on suspend the node files a
   * request per gated tool; on resume it executes the tools the engine reports as
   * approved. The engine becomes the source of truth (a human resolves it out of
   * band) instead of the `__approvedTools` channel.
   */
  approvalEngine?: ApprovalEngine;
  label?: string;
};

/** Config for {@link GraphBuilder.toolNode}. */
export type ToolNodeConfig = {
  tools: ToolRegistry;
  /** Execute all tool calls concurrently instead of sequentially. */
  parallel?: boolean;
  label?: string;
};

/**
 * A tool's name plus its TS `execute` fn — the data the Rust bridge needs to back a
 * `jsToolName` with a JS callback. The Rust engine never imports the tool registry;
 * it calls this `execute` over the napi seam (`on_node` with `kind:"tool"`).
 */
export type RustToolBinding = {
  name: string;
  execute: (input: unknown) => Promise<unknown>;
};

/**
 * The serializable shape of an agent node, plus its JS-backed tool executes, that
 * the Rust engine bridge consumes (see `EngineSpec.agents` / `jsToolNames`). It is a
 * pure projection of {@link AgentNodeConfig} — the system prompt is the *resolved*
 * string (never a registry reference), since the bridge has no prompt registry.
 *
 * The LLM gateway itself is **not** carried: the Rust agent path builds its own
 * gateway (env adapters or a deterministic mock). A graph whose agents rely on a
 * specific TS `AgentNodeConfig.llm` therefore keeps its semantics only on the TS
 * engine; the Rust path is opt-in for agents (see `CompiledGraph`).
 */
export type RustAgentConfig = {
  provider: string;
  model?: string;
  /**
   * Abstract capability tier carried to the Rust `AgentSpec.tier`. When set with no
   * explicit `model`, the Rust bridge resolves the concrete model via `ModelPolicy`
   * against the process env. An explicit `model` always wins.
   */
  tier?: ModelTier;
  /** Resolved system prompt string. */
  system?: string;
  toolNames: string[];
  maxIterations?: number;
  suspendForApproval: boolean;
  /** Tools (by name) requiring approval — those marked `requiresApproval`. */
  approvalToolNames: string[];
  outputChannel: string;
  /** JS-backed tool executes, one per tool in the registry. */
  toolBindings: RustToolBinding[];
  /**
   * SDK-only (never serialized to the wire): whether this agent node was configured
   * with a TS {@link ApprovalEngine}. The engine-backed approval flow — filing a
   * request per gated tool and reading the engine's decision on resume — lives in the
   * TS `createAgentNodeHandler`; the Rust agent path does not invoke it. So a graph
   * with an `approvalEngine` keeps its agent nodes on the TS engine under `auto`.
   */
  usesApprovalEngine: boolean;
};

/**
 * The governance binding an agent node contributes to {@link CompiledGraph}: the
 * (optional) {@link ApprovalEngine} a human resolves requests through, the principal
 * that *requests* approvals on this node's behalf (`config.name ?? nodeId`, the same
 * `requestedBy` the node files requests under), and the names of its approval-gated
 * tools. {@link CompiledGraph.approveAndResume} uses it to (a) approve the matching
 * pending engine requests before resuming on the TS path, and (b) stamp each granted
 * tool's `requestedBy` for the Rust engine's no-self-approval guard-rail.
 */
export type AgentApprovalBinding = {
  approvalEngine?: ApprovalEngine;
  requestedBy: string;
  approvalToolNames: string[];
};

/** Project an {@link AgentNodeConfig} into its {@link AgentApprovalBinding}. */
export const toAgentApprovalBinding = (
  nodeId: string,
  config: AgentNodeConfig
): AgentApprovalBinding => ({
  approvalEngine: config.approvalEngine,
  requestedBy: config.name ?? nodeId,
  approvalToolNames: approvalToolNamesOf(config.tools)
});

/** Pull every tool's name + `execute` out of a registry, for the Rust tool seam. */
const toolBindingsOf = (tools: ToolRegistry | undefined): RustToolBinding[] => {
  if (tools === undefined) {
    return [];
  }
  return tools.list().map((definition) => {
    const resolved = tools.resolve(definition.id);
    const execute = resolved?.handler ?? (async () => ({}));
    return { name: definition.name, execute: (input: unknown) => execute(input) };
  });
};

/** Tool names whose definition is flagged `requiresApproval`. */
const approvalToolNamesOf = (tools: ToolRegistry | undefined): string[] => {
  if (tools === undefined) {
    return [];
  }
  return tools
    .list()
    .filter((definition) => definition.requiresApproval === true)
    .map((definition) => definition.name);
};

/**
 * Project an {@link AgentNodeConfig} into the {@link RustAgentConfig} the Rust engine
 * bridge consumes. Resolves the system prompt to a concrete string and pulls the tool
 * names / approval flags / executes out of the registry. Pure — no LLM call.
 */
export const toRustAgentConfig = (nodeId: string, config: AgentNodeConfig): RustAgentConfig => {
  const { registry, id, version } = resolvePrompt(nodeId, config.prompt);
  let system: string | undefined;
  try {
    system = registry.get(id, version).system;
  } catch {
    system = undefined;
  }
  return {
    provider: config.provider ?? "anthropic",
    model: config.model,
    tier: config.tier,
    system,
    toolNames: config.tools?.list().map((definition) => definition.name) ?? [],
    maxIterations: config.maxIterations,
    suspendForApproval: config.suspendForApproval === true,
    approvalToolNames: approvalToolNamesOf(config.tools),
    outputChannel: config.outputChannel ?? DEFAULT_AGENT_OUTPUT_CHANNEL,
    toolBindings: toolBindingsOf(config.tools),
    usesApprovalEngine: config.approvalEngine !== undefined
  };
};

const resolvePrompt = (
  nodeId: string,
  prompt: AgentPromptSource
): { registry: PromptRegistry; id: string; version?: string } => {
  if ("system" in prompt) {
    // Even inline prompts are referenced by id, never hardcoded into the agent —
    // we register the string under a deterministic id and hand back a reference.
    const registry = new InMemoryPromptRegistry();
    const id = `sdk.agent.${nodeId}.system`;
    registry.register({ id, version: "1.0.0", system: prompt.system });
    return { registry, id, version: "1.0.0" };
  }
  return { registry: prompt.registry, id: prompt.id, version: prompt.version };
};

/** Config for {@link streamAgentTokens}. */
export type StreamAgentConfig = {
  llm: LLMGateway;
  prompt: AgentPromptSource;
  provider?: LLMProvider;
  model?: string;
};

/**
 * Stream an agent's reply token by token through the gateway's `stream()`. This is
 * the single-turn (no-tools) path — ideal for a chat UI that wants live output.
 * Yields text deltas as they arrive and returns when the provider signals done.
 *
 * ```ts
 * for await (const delta of streamAgentTokens({ llm, prompt: { system } }, "Bonjour ?")) {
 *   process.stdout.write(delta);
 * }
 * ```
 */
export async function* streamAgentTokens(config: StreamAgentConfig, input: unknown): AsyncIterable<string> {
  const { registry, id, version } = resolvePrompt("stream", config.prompt);
  const system = registry.get(id, version).system;

  const stream = config.llm.stream({
    provider: config.provider ?? "anthropic",
    model: config.model ?? "claude-opus-4-8",
    system,
    messages: [{ role: "user", content: typeof input === "string" ? input : JSON.stringify(input) }]
  });

  for await (const chunk of stream) {
    if (chunk.delta.length > 0) {
      yield chunk.delta;
    }
    if (chunk.done) {
      return;
    }
  }
}

/**
 * Build the handler for an agent node: a {@link ReActAgent} driven by the given
 * LLM gateway. The agent's result is written to `outputChannel`; route on its
 * `requiresHumanReview` flag (e.g. a conditional edge into a human gate) to keep
 * sensitive actions behind approval — an agent never self-approves.
 */
const channelArray = (channels: Record<string, unknown>, key: string): string[] => {
  const value = channels[key];
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
};

/** A tool subject is `{ description: "tool:<name>" }`; pull the tool name back out. */
const subjectToolName = (subject: { description: string } | { [key: string]: unknown }): string | undefined => {
  const description = (subject as { description?: unknown }).description;
  return typeof description === "string" && description.startsWith(TOOL_SUBJECT_PREFIX)
    ? description.slice(TOOL_SUBJECT_PREFIX.length)
    : undefined;
};

/**
 * The set of approval-gated tools the agent may now run. The channel path covers
 * `approveAndResume`; the engine path covers a real {@link ApprovalEngine} decision
 * resolved out of band — we look up the request ids stashed at suspend time.
 */
const resolveApprovedTools = async (
  channels: Record<string, unknown>,
  engine: ApprovalEngine | undefined
): Promise<string[]> => {
  const approved = new Set(channelArray(channels, APPROVED_TOOLS_CHANNEL));
  if (engine !== undefined) {
    for (const rawId of channelArray(channels, APPROVAL_IDS_CHANNEL)) {
      const request = await engine.getById(rawId as ApprovalId);
      if (request?.status === "approved") {
        const toolName = subjectToolName(request.subject);
        if (toolName !== undefined) {
          approved.add(toolName);
        }
      }
    }
  }
  return [...approved];
};

/**
 * Resolve the concrete `{ provider, model }` an agent node runs on, honouring the
 * explicit-override precedence: an explicit `model`/`provider` always wins; a `tier`
 * (with no explicit model) maps through the {@link ModelPolicy} against the providers
 * available in the current env. This keeps the TS fallback path consistent with the
 * Rust bridge's `resolve_agent_model` — "I only have Mistral" resolves every tier to
 * the mistral column. With neither tier nor a usable provider, returns the config's
 * explicit values (the {@link ReActAgent} then applies its own defaults).
 */
export const resolveAgentModel = (
  config: Pick<AgentNodeConfig, "provider" | "model" | "tier">
): { provider?: LLMProvider; model?: string } => {
  // No tier, or an explicit model already pins the choice: keep what was given so the
  // explicit override wins and the ReActAgent default applies when unset.
  if (config.tier === undefined || config.model !== undefined) {
    return { provider: config.provider, model: config.model };
  }
  const policy = new ModelPolicy();
  const available = policy.availableFromEnv();
  const choice = policy.resolve(config.tier, available, { provider: config.provider });
  return { provider: choice.provider, model: choice.model };
};

export const createAgentNodeHandler = (nodeId: string, config: AgentNodeConfig): NodeHandler => {
  const { registry, id, version } = resolvePrompt(nodeId, config.prompt);
  const outputChannel = config.outputChannel ?? DEFAULT_AGENT_OUTPUT_CHANNEL;
  const { provider, model } = resolveAgentModel(config);

  return async (input, state, context) => {
    const channels = state.channels as Record<string, unknown>;
    const approvedToolNames = await resolveApprovedTools(channels, config.approvalEngine);

    const agent = new ReActAgent<unknown>({
      id: nodeId as AgentId,
      name: config.name ?? nodeId,
      description: config.description ?? `agent node ${nodeId}`,
      llm: config.llm,
      tools: config.tools,
      provider,
      model,
      maxIterations: config.maxIterations,
      promptRegistry: registry,
      promptId: id,
      promptVersion: version,
      approvedToolNames
    });

    const result = await agent.run(input, state, {
      memory: context.memory,
      workingMemory: { shortTerm: [], longTerm: context.memory }
    });

    // Native suspend-on-approval: stop the whole run cleanly (a checkpointed,
    // resumable suspension) rather than leaving routing to the caller. The pending
    // result — including its approvalRequests — is persisted to the output channel.
    if (config.suspendForApproval === true && result.requiresHumanReview) {
      const patch: Record<string, unknown> = { [outputChannel]: result };

      // File one ApprovalEngine request per gated tool and stash the ids, so resume
      // can ask the engine which were approved. The agent is the requester — a human
      // (a different principal) resolves it, which the engine enforces.
      if (config.approvalEngine !== undefined) {
        const ids: string[] = [];
        for (const request of result.approvalRequests) {
          const created = await config.approvalEngine.request({
            runId: state.runId as RunId,
            nodeId: state.currentNodeId as NodeId,
            requestedBy: config.name ?? nodeId,
            subject: request.subject
          });
          ids.push(String(created.id));
        }
        patch[APPROVAL_IDS_CHANNEL] = ids;
      }

      throw new DynamicInterrupt(AGENT_APPROVAL_INTERRUPT, patch);
    }

    return { [outputChannel]: result };
  };
};

/**
 * Build the handler for a tool node: executes the tool calls emitted by the last
 * AI message in the `messages` channel. Tools flagged `requiresApproval` suspend
 * the run via a dynamic interrupt instead of executing.
 */
export const createToolNodeHandler = (config: ToolNodeConfig): NodeHandler =>
  createToolNode(config.tools, { parallel: config.parallel });
