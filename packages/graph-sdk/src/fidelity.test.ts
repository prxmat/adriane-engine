import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  createGraph,
  DefaultLLMGateway,
  InMemoryToolRegistry,
  MockLLMProviderAdapter,
  rustEngineAvailable,
  type LLMGateway,
  type RunEvent,
  type RunId,
  type ToolId
} from "./index.js";

/**
 * Cross-engine **fidelity** suite: it runs the *same* graphs on the TypeScript engine
 * (`ADRIANE_SDK_ENGINE=ts`) and the Rust engine (`=rust`) and asserts their
 * **observable** results match. This is the gate that justifies `auto` routing agent
 * and JS-handler graphs to Rust by default: if the structural contract holds across
 * engines on the deterministic mock gateway, the flip is safe.
 *
 * What "observable" means here (the contract the two engines share):
 * - final run **status** (completed / suspended) and suspend → approve/resume → done,
 * - that an approval-gated tool does **not** execute before approval and **does** after
 *   (a side-effect counter; the agent never self-approves),
 * - the **channel update** a JS node applies (same value on both engines),
 * - **conditional-edge routing** (same `currentNodeId` after the predicate runs),
 * - the run-lifecycle **event** vocabulary forwarded to `onEvent`.
 *
 * What it deliberately does **not** assert (documented divergences, not bugs):
 * - the agent `AgentResult.reasoning` text — the Rust agent builds its own gateway, so
 *   the mock emits different strings than the TS `AgentNodeConfig.llm`,
 * - the exact tool-call **count** — the two gateways script the agent loop differently
 *   (so the loop iterates a different number of times), only the *gated-then-executed*
 *   structure is asserted.
 *
 * Runs only when the native addon is present (so the `rust` half is real); otherwise
 * the whole suite skips and stays green on a machine with no addon.
 */
const describeIfRust = rustEngineAvailable() ? describe : describe.skip;

const passthrough = { parse: (value: unknown) => value };

/** A TS mock gateway that always asks to call `toolName` (single response, replayed). */
const toolCallGateway = (toolName: string): LLMGateway => {
  const gateway = new DefaultLLMGateway();
  gateway.registerAdapter(
    new MockLLMProviderAdapter({
      provider: "anthropic",
      response: {
        content: "",
        toolCalls: [{ id: "tu1", name: toolName, input: {} }],
        stopReason: "tool_use",
        usage: { promptTokens: 0, completionTokens: 0 },
        model: "mock",
        provider: "anthropic"
      }
    })
  );
  return gateway;
};

/** Distinct event types seen, for a stable set comparison. */
const eventTypes = (events: RunEvent[]): string[] => [...new Set(events.map((event) => event.type))].sort();

describeIfRust("@adriane-ai/graph-sdk — TS vs Rust engine fidelity", () => {
  // Provider keys that would steer the Rust agent path off its deterministic mock onto
  // a real provider. Force them off so both engines are reproducible.
  const PROVIDER_KEYS = ["MISTRAL_API_KEY", "ANTHROPIC_API_KEY", "ADRIANE_USE_OLLAMA"] as const;
  const saved: Record<string, string | undefined> = {};

  beforeEach(() => {
    saved.ADRIANE_SDK_ENGINE = process.env.ADRIANE_SDK_ENGINE;
    for (const key of PROVIDER_KEYS) {
      saved[key] = process.env[key];
      delete process.env[key];
    }
  });

  afterEach(() => {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) {
        delete process.env[key];
      } else {
        process.env[key] = value;
      }
    }
  });

  /** Run the body once per engine, returning the per-engine observation. */
  const onEachEngine = async <T>(
    build: () => Promise<T>
  ): Promise<{ ts: T; rust: T }> => {
    process.env.ADRIANE_SDK_ENGINE = "ts";
    const ts = await build();
    process.env.ADRIANE_SDK_ENGINE = "rust";
    const rust = await build();
    return { ts, rust };
  };

  it("an agentNode with a gated tool: gated before approval, executed after, on both engines", async () => {
    const observe = async (engine: string) => {
      // A side-effect counter proves the tool runs only after approval. The tool's TS
      // `execute` is bridged to Rust over the (async) napi seam, so it runs on both.
      let toolCalls = 0;
      const tools = new InMemoryToolRegistry();
      tools.register(
        {
          id: "refund" as ToolId,
          name: "refund",
          description: "Issues a refund. Sensitive.",
          inputSchema: passthrough,
          outputSchema: passthrough,
          permissions: ["payments:write"],
          requiresApproval: true,
          jsonSchema: { type: "object" }
        },
        async () => {
          toolCalls += 1;
          return { ok: true };
        }
      );

      const events: RunEvent[] = [];
      const app = createGraph({ name: "fidelity-gated-agent" })
        .agentNode("assistant", {
          llm: toolCallGateway("refund"),
          prompt: { system: "Use tools when needed." },
          tools,
          suspendForApproval: true,
          maxIterations: 4
        })
        .compile();
      app.onEvent((event) => events.push(event));

      const runId = `run_fid_gated_${engine}` as RunId;
      const suspended = await app.run({}, { runId });
      const callsAtSuspend = toolCalls;
      const done = await app.approveAndResume(suspended.runId, { approvedTools: ["refund"] });

      return {
        usesRust: app.usesRustEngine,
        suspendedStatus: suspended.status,
        callsAtSuspend,
        doneStatus: done.status,
        toolRanAfterApproval: toolCalls > 0,
        events: eventTypes(events)
      };
    };

    const { ts, rust } = await onEachEngine(() => observe(process.env.ADRIANE_SDK_ENGINE ?? "ts"));

    // The engine actually used differs (that is the point).
    expect(ts.usesRust).toBe(false);
    expect(rust.usesRust).toBe(true);

    // Observable structural contract is identical across engines.
    expect(ts.suspendedStatus).toBe("suspended");
    expect(rust.suspendedStatus).toBe("suspended");
    expect(ts.callsAtSuspend).toBe(0); // gated: no execution before approval
    expect(rust.callsAtSuspend).toBe(0);
    expect(ts.doneStatus).toBe("completed");
    expect(rust.doneStatus).toBe("completed");
    expect(ts.toolRanAfterApproval).toBe(true); // executed once granted
    expect(rust.toolRanAfterApproval).toBe(true);
    expect(rust.events).toEqual(ts.events); // same lifecycle vocabulary forwarded
    expect(ts.events).toContain("run_suspended");
    expect(ts.events).toContain("run_completed");
  });

  it("a custom JS node applies the same channel update on both engines", async () => {
    const observe = async (engine: string) => {
      const app = createGraph({ name: "fidelity-js-node" })
        .channel("count", { type: "number", default: 0 })
        .channel("doubled", { type: "number", default: 0 })
        // An async JS handler that reads a channel and writes a derived one. On Rust it
        // is invoked over the `on_node` seam (the seam awaits the returned Promise).
        .node("double", async (_input, state) => ({ doubled: (state.channels.count as number) * 2 }))
        .compile();

      const result = await app.run({ count: 21 }, { runId: `run_fid_js_${engine}` as RunId });
      return {
        usesRust: app.usesRustEngine,
        status: result.status,
        doubled: (result.channels as Record<string, unknown>).doubled
      };
    };

    const { ts, rust } = await onEachEngine(() => observe(process.env.ADRIANE_SDK_ENGINE ?? "ts"));

    expect(ts.usesRust).toBe(false);
    expect(rust.usesRust).toBe(true);
    expect(ts.status).toBe("completed");
    expect(rust.status).toBe("completed");
    // The JS node's channel update round-trips identically across the seam.
    expect(ts.doubled).toBe(42);
    expect(rust.doubled).toBe(42);
    expect(rust.doubled).toBe(ts.doubled);
  });

  it("a named conditional edge routes identically on both engines", async () => {
    const observe = async (engine: string) => {
      const app = createGraph({ name: "fidelity-conditional" })
        .channel("ready", { type: "boolean", default: false })
        .humanGate("gate")
        .humanGate("second")
        // A synchronous predicate over the channels; bridged to Rust via on_condition.
        .conditionalEdge("gate", "second", "isReady", (state) => Boolean(state.channels.ready))
        .compile();

      const runId = `run_fid_cond_${engine}` as RunId;
      const suspended = await app.run({ ready: true }, { runId });
      const resumed = await app.resume(suspended.runId);
      return {
        usesRust: app.usesRustEngine,
        suspendedNode: suspended.currentNodeId,
        resumedNode: resumed.currentNodeId,
        resumedStatus: resumed.status
      };
    };

    const { ts, rust } = await onEachEngine(() => observe(process.env.ADRIANE_SDK_ENGINE ?? "ts"));

    expect(ts.usesRust).toBe(false);
    expect(rust.usesRust).toBe(true);
    // First gate suspends; the predicate (ready=true) routes past it to the second gate.
    expect(ts.suspendedNode).toBe("gate");
    expect(rust.suspendedNode).toBe("gate");
    expect(ts.resumedNode).toBe("second");
    expect(rust.resumedNode).toBe("second");
    expect(rust.resumedNode).toBe(ts.resumedNode); // condition routing matches
    expect(ts.resumedStatus).toBe("suspended");
    expect(rust.resumedStatus).toBe("suspended");
  });
});
