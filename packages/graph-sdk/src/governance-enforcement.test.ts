import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
// Import the in-memory engine directly (not the package index) so the test never
// pulls the Pg engine and its `db`/`pg` dependency chain.
import { InMemoryApprovalEngine } from "../../approval-engine/src/in-memory-approval-engine.js";

import {
  APPROVAL_IDS_CHANNEL,
  createGraph,
  DefaultLLMGateway,
  InMemoryToolRegistry,
  isCatalogGraph,
  MockLLMProviderAdapter,
  runCatalogGraph,
  rustEngineAvailable,
  type GraphDefinition,
  type LLMGateway,
  type ToolId
} from "./index.js";

const passthrough = { parse: (value: unknown) => value };

/** A gateway whose single response calls `toolName` (a `tool_use`), so the agent gates. */
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

const gatedToolRegistry = (name: string, handler: () => Promise<unknown>): InMemoryToolRegistry => {
  const tools = new InMemoryToolRegistry();
  tools.register(
    {
      id: name as ToolId,
      name,
      description: `Issues a ${name}.`,
      inputSchema: passthrough,
      outputSchema: passthrough,
      permissions: ["payments:write"],
      requiresApproval: true,
      jsonSchema: { type: "object" }
    },
    handler
  );
  return tools;
};

// ---------------------------------------------------------------------------
// TS path: approveAndResume resolves through the engine under `resolvedBy`.
// ---------------------------------------------------------------------------

describe("@adriane/graph-sdk governance — approveAndResume(resolvedBy) on the TS engine", () => {
  let savedEngine: string | undefined;
  beforeEach(() => {
    savedEngine = process.env.ADRIANE_SDK_ENGINE;
    process.env.ADRIANE_SDK_ENGINE = "ts"; // pin: assert the TS-path enforcement mirror
  });
  afterEach(() => {
    if (savedEngine === undefined) {
      delete process.env.ADRIANE_SDK_ENGINE;
    } else {
      process.env.ADRIANE_SDK_ENGINE = savedEngine;
    }
  });

  it("approves the pending engine request under resolvedBy, then executes the tool", async () => {
    const engine = new InMemoryApprovalEngine();
    const handler = vi.fn(async () => ({ ok: true }));

    const app = createGraph({ name: "gov-approve" })
      .agentNode("assistant", {
        llm: toolCallGateway("refund"),
        prompt: { system: "Use tools when needed." },
        tools: gatedToolRegistry("refund", handler),
        suspendForApproval: true,
        approvalEngine: engine,
        maxIterations: 2
      })
      .compile();

    const suspended = await app.run({}, { runId: "run_gov_1" as never });
    expect(suspended.status).toBe("suspended");
    expect(handler).not.toHaveBeenCalled();

    // The agent filed a pending request (requestedBy = the node).
    const pending = await engine.getPending(suspended.runId);
    expect(pending).toHaveLength(1);
    expect(pending[0]?.requestedBy).toBe("assistant");
    expect(pending[0]?.status).toBe("pending");

    // approveAndResume resolves it THROUGH the engine under a distinct human principal.
    const done = await app.approveAndResume(suspended.runId, {
      approvedTools: ["refund"],
      resolvedBy: "alice"
    });

    expect(done.status).toBe("completed");
    expect(handler).toHaveBeenCalled();
    const resolved = await engine.getById(pending[0]!.id);
    expect(resolved?.status).toBe("approved");
    expect(resolved?.resolvedBy).toBe("alice");
  });

  it("writes the approved tools into __approvedTools sorted + de-duplicated (deterministic)", async () => {
    const engine = new InMemoryApprovalEngine();
    const app = createGraph({ name: "gov-determinism" })
      .agentNode("assistant", {
        llm: toolCallGateway("refund"),
        prompt: { system: "Use tools when needed." },
        tools: gatedToolRegistry("refund", async () => ({ ok: true })),
        suspendForApproval: true,
        approvalEngine: engine,
        maxIterations: 2
      })
      .compile();

    const suspended = await app.run({}, { runId: "run_gov_det" as never });
    expect(suspended.status).toBe("suspended");

    // Spy on the runtime's state update to capture the EXACT channel payload written
    // before the agent re-runs (and consumes it). The names must be sorted + unique
    // regardless of the caller's ordering and duplicates.
    const update = vi.spyOn(app.engine, "updateState");
    await app.approveAndResume(suspended.runId, {
      approvedTools: ["wire", "refund", "refund", "ach"],
      resolvedBy: "alice"
    });

    const written = update.mock.calls
      .map((call) => (call[1] as Record<string, unknown>)["__approvedTools"])
      .find((value): value is string[] => Array.isArray(value));
    expect(written).toEqual(["ach", "refund", "wire"]);
    update.mockRestore();
  });

  it("refuses self-approval: resolvedBy equal to the requester throws via the engine", async () => {
    const engine = new InMemoryApprovalEngine();
    const handler = vi.fn(async () => ({ ok: true }));

    const app = createGraph({ name: "gov-self" })
      .agentNode("assistant", {
        llm: toolCallGateway("refund"),
        prompt: { system: "Use tools when needed." },
        tools: gatedToolRegistry("refund", handler),
        suspendForApproval: true,
        approvalEngine: engine,
        maxIterations: 2
      })
      .compile();

    const suspended = await app.run({}, { runId: "run_gov_2" as never });
    expect(suspended.status).toBe("suspended");

    // The requester is the node "assistant"; granting as "assistant" is self-approval.
    await expect(
      app.approveAndResume(suspended.runId, { approvedTools: ["refund"], resolvedBy: "assistant" })
    ).rejects.toThrow();

    // The request is still pending and the gated tool never executed.
    const pending = await engine.getPending(suspended.runId);
    expect(pending).toHaveLength(1);
    expect(handler).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Catalog (Rust) path: emission via the seam, non-duplication, determinism.
// ---------------------------------------------------------------------------

/** A catalog graph: a single gated agent node carried entirely in node.metadata.agent. */
const catalogGatedGraph = (): GraphDefinition =>
  ({
    id: "gov-catalog",
    version: "0.0.0",
    name: "gov-catalog",
    channels: {
      agentResult: { type: "agentResult", reducer: "replace" },
      __approvedTools: { type: "string[]", reducer: "replace", default: [] },
      __approvalIds: { type: "string[]", reducer: "replace", default: [] }
    },
    nodes: [
      {
        id: "assistant",
        type: "agent",
        label: "assistant",
        metadata: {
          agent: {
            provider: "anthropic",
            toolNames: ["refund"],
            suspendForApproval: true,
            approvalToolNames: ["refund"],
            outputChannel: "agentResult"
          }
        }
      }
    ],
    edges: [],
    entryNodeId: "assistant"
  }) as unknown as GraphDefinition;

const rustOnly = rustEngineAvailable() ? describe : describe.skip;

rustOnly("@adriane/graph-sdk governance — catalog seam emission (Rust engine)", () => {
  it("recognizes the gated agent graph as a catalog graph", () => {
    expect(isCatalogGraph(catalogGatedGraph())).toBe(true);
  });

  it("files exactly one ApprovalEngine request per gated tool and stashes its id", async () => {
    const engine = new InMemoryApprovalEngine();
    const outcome = await runCatalogGraph(catalogGatedGraph(), {
      runId: "run_cat_1" as never,
      approvalEngine: engine
    });

    expect(outcome.status).toBe("suspended");

    // Emission happened in the SEAM (control-plane authority), not duplicated: exactly
    // one pending request, filed under the node id as requester.
    const pending = await engine.getPending("run_cat_1" as never);
    expect(pending).toHaveLength(1);
    expect(pending[0]?.requestedBy).toBe("assistant");
    expect(pending[0]?.subject).toMatchObject({ description: "tool:refund" });

    // The engine id is stashed in the run's __approvalIds channel.
    const ids = (outcome.state.channels as Record<string, unknown>)[APPROVAL_IDS_CHANNEL];
    expect(Array.isArray(ids)).toBe(true);
    expect(ids as string[]).toHaveLength(1);
    expect((ids as string[])[0]).toBe(String(pending[0]!.id));
  });

  it("does not double-file: emission happens in the seam, never also via a TS handler", async () => {
    // The catalog path runs the agent NATIVELY on Rust — there is no TS
    // createAgentNodeHandler in play — so the only emission point is the seam. One
    // suspended run yields exactly one pending request (catalog XOR fallback emission).
    const engine = new InMemoryApprovalEngine();
    const outcome = await runCatalogGraph(catalogGatedGraph(), {
      runId: "run_cat_2" as never,
      approvalEngine: engine
    });
    expect(outcome.status).toBe("suspended");
    const pending = await engine.getPending("run_cat_2" as never);
    expect(pending).toHaveLength(1);
    const ids = (outcome.state.channels as Record<string, unknown>)[APPROVAL_IDS_CHANNEL];
    expect((ids as string[]).length).toBe(1); // one id stashed, matching the one request
  });

  it("an ungoverned catalog run (no engine) files nothing", async () => {
    const engine = new InMemoryApprovalEngine();
    const outcome = await runCatalogGraph(catalogGatedGraph(), {
      runId: "run_cat_3" as never
      // no approvalEngine — legacy channel-only behaviour
    });
    expect(outcome.status).toBe("suspended");
    expect((await engine.getPending("run_cat_3" as never)).length).toBe(0);
    const ids = (outcome.state.channels as Record<string, unknown>)[APPROVAL_IDS_CHANNEL];
    expect(ids === undefined || (ids as string[]).length === 0).toBe(true);
  });
});
