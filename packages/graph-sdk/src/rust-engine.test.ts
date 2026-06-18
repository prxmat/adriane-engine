import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  createGraph,
  DefaultLLMGateway,
  InMemoryToolRegistry,
  rustEngineAvailable,
  type AgentResult,
  type RunEvent,
  type ToolId
} from "./index.js";

/**
 * These tests exercise the **Rust engine** execution path of `CompiledGraph` through
 * the `@adriane-ai/napi` bridge. They run only when the native addon is present (built
 * via `scripts/build-napi.sh`); otherwise they are skipped, so the suite stays green
 * on a machine with no addon.
 *
 * They are forced onto Rust with `ADRIANE_SDK_ENGINE=rust`. The Rust agent path uses
 * the engine's own gateway (a deterministic mock with no provider keys present). Since
 * Phase F the napi seam awaits async JS callbacks, so JS tool `execute` fns now bridge
 * by default — the Rust agent calls back into the TS tool over the seam. We assert the
 * *structural* contract (suspend / approve / complete + Rust-only markers), which is
 * what the Rust path preserves; the agent `reasoning` text is gateway-specific and not
 * asserted here.
 */
const describeIfRust = rustEngineAvailable() ? describe : describe.skip;

const passthrough = { parse: (value: unknown) => value };

describeIfRust("@adriane-ai/graph-sdk — Rust engine execution", () => {
  // Keys that would steer the Rust agent path to a real provider instead of the
  // deterministic mock. We force them off for the duration of these tests so the
  // agent path is reproducible regardless of the developer's environment.
  const PROVIDER_KEYS = ["MISTRAL_API_KEY", "ANTHROPIC_API_KEY", "ADRIANE_USE_OLLAMA"] as const;
  const saved: Record<string, string | undefined> = {};

  beforeEach(() => {
    saved.ADRIANE_SDK_ENGINE = process.env.ADRIANE_SDK_ENGINE;
    process.env.ADRIANE_SDK_ENGINE = "rust";
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

  it("runs a simple agent node to completion ON THE RUST ENGINE (mock gateway)", async () => {
    const events: RunEvent[] = [];
    const app = createGraph({ name: "rust-agent-complete" })
      .agentNode("assistant", {
        // The TS gateway is unused on the Rust path (the engine builds its own mock);
        // we still supply one so the graph also runs on the TS engine if forced there.
        llm: new DefaultLLMGateway(),
        prompt: { system: "You are a brief assistant." },
        maxIterations: 2
      })
      .compile();

    // Marker 1: the graph is wired to the Rust engine, not the TS runtime.
    expect(app.usesRustEngine).toBe(true);

    const unsubscribe = app.onEvent((event) => events.push(event));
    const result = await app.run({ question: "hi" }, { runId: "run_rust_done" as never });
    unsubscribe();

    expect(result.status).toBe("completed");
    // Marker 2: lifecycle events were forwarded from Rust through the bridge.
    expect(events.some((event) => event.type === "node_started")).toBe(true);
    expect(events.some((event) => event.type === "run_completed")).toBe(true);
    // The agent's result landed in the default output channel.
    const agentResult = (result.channels as Record<string, AgentResult>).agentResult;
    expect(agentResult).toBeDefined();
  });

  it("suspends a gated agent then resumes to completion via approveAndResume ON RUST", async () => {
    // A tool flagged `requiresApproval`; with `suspendForApproval` the Rust agent node
    // raises a dynamic interrupt and the run suspends *before* the tool runs. Once
    // approved, the Rust agent calls the tool's TS `execute` back over the (now async)
    // napi seam — the structural suspend → approve → complete contract holds.
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
      async () => ({ ok: true })
    );

    const events: RunEvent[] = [];
    const app = createGraph({ name: "rust-agent-gated" })
      .agentNode("assistant", {
        llm: new DefaultLLMGateway(),
        prompt: { system: "Use tools when needed." },
        tools,
        suspendForApproval: true,
        maxIterations: 4
      })
      .compile();

    expect(app.usesRustEngine).toBe(true);
    app.onEvent((event) => events.push(event));

    const suspended = await app.run({}, { runId: "run_rust_gate" as never });
    expect(suspended.status).toBe("suspended");
    // Marker: Rust forwarded a run_suspended event for the gated tool.
    expect(events.some((event) => event.type === "run_suspended")).toBe(true);

    const done = await app.approveAndResume(suspended.runId, { approvedTools: ["refund"] });
    expect(done.status).toBe("completed");
    expect(events.some((event) => event.type === "run_completed")).toBe(true);
  });

  it("routes a named conditional edge through the Rust engine (sync predicate seam)", async () => {
    // A pure human-gate + condition graph (also what `auto` would pick). It exercises
    // the on_condition seam — a synchronous boolean round-trip — end to end: the run
    // suspends at the first gate, and on resume the predicate routes it to the second.
    let predicateState: Record<string, unknown> | undefined;
    const app = createGraph({ name: "rust-conditional" })
      .channel("ready", { type: "boolean", default: false })
      .humanGate("gate")
      .humanGate("second")
      .conditionalEdge("gate", "second", "isReady", (state) => {
        predicateState = state.channels as Record<string, unknown>;
        return Boolean((state.channels as Record<string, boolean>).ready);
      })
      .compile();

    expect(app.usesRustEngine).toBe(true);

    const suspended = await app.run({ ready: true }, { runId: "run_rust_cond" as never });
    expect(suspended.status).toBe("suspended");
    expect(suspended.currentNodeId).toBe("gate");

    const resumed = await app.resume(suspended.runId);
    // The Rust on_condition seam evaluated the TS predicate against the channel state
    // and routed past the gate to the second gate (which suspends again).
    expect(predicateState?.ready).toBe(true);
    expect(resumed.currentNodeId).toBe("second");
  });
});
