import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createGraph, rustEngineAvailable, type RunId, type StreamEvent } from "./index.js";

/**
 * Subgraph nesting + streaming through the SDK. The subgraph + streaming behaviour
 * runs end-to-end on the **Rust engine** (the canonical runtime) when the native
 * addon is present, and a parity slice runs on the **TS engine** (forced) so the
 * feature holds on both paths. ADR 0008.
 */

/** Child graph that doubles its mapped-in `in` channel into `out`. */
const buildDoubler = () =>
  createGraph({ name: "double", id: "double" })
    .channel("in", { type: "number", default: 0 })
    .channel("out", { type: "number", default: 0 })
    .node("calc", async (_input, state) => ({ out: (state.channels.in ?? 0) * 2 }));

/** Child graph with an internal human gate: draft → gate → publish. */
const buildGatedChild = () =>
  createGraph({ name: "gated-child", id: "gated-child" })
    .channel("drafted", { type: "boolean", default: false })
    .channel("out", { type: "string", default: "" })
    .node("c_draft", async () => ({ drafted: true }))
    .humanGate("c_gate")
    .node("c_publish", async () => ({ out: "published" }))
    .edge("c_draft", "c_gate")
    .edge("c_gate", "c_publish");

const withEngine = (engine: "rust" | "ts") => {
  let saved: string | undefined;
  beforeEach(() => {
    saved = process.env.ADRIANE_SDK_ENGINE;
    process.env.ADRIANE_SDK_ENGINE = engine;
  });
  afterEach(() => {
    if (saved === undefined) {
      delete process.env.ADRIANE_SDK_ENGINE;
    } else {
      process.env.ADRIANE_SDK_ENGINE = saved;
    }
  });
};

const describeIfRust = rustEngineAvailable() ? describe : describe.skip;

describeIfRust("@adriane-ai/graph-sdk — subgraphs + streaming (Rust engine)", () => {
  withEngine("rust");

  it("runs a subgraph node mapping channels in and out", async () => {
    const app = createGraph({ name: "parent-double" })
      .channel("x", { type: "number", default: 21 })
      .channel("y", { type: "number", default: 0 })
      .subgraph("sub", buildDoubler(), {
        inputMapping: { in: "x" },
        outputMapping: { y: "out" }
      })
      .compile();

    expect(app.usesRustEngine).toBe(true);
    const result = await app.run({ x: 21 }, { runId: "run_sub_double" as RunId });
    expect(result.status).toBe("completed");
    expect(result.channels.y).toBe(42);
  });

  it("suspends when a subgraph hits an internal human gate, then resumes", async () => {
    const app = createGraph({ name: "parent-gated" })
      .channel("result", { type: "string", default: "" })
      .channel("done", { type: "boolean", default: false })
      .subgraph("sub", buildGatedChild(), { outputMapping: { result: "out" } })
      .node("after", async () => ({ done: true }))
      .edge("sub", "after")
      .compile();

    expect(app.usesRustEngine).toBe(true);
    const suspended = await app.run({}, { runId: "run_sub_gate" as RunId });
    expect(suspended.status).toBe("suspended");

    const resumed = await app.resume(suspended.runId);
    expect(resumed.status).toBe("completed");
    expect(resumed.channels.result).toBe("published");
    expect(resumed.channels.done).toBe(true);
  });

  it("streams per-node updates with a defined nodeId (camelCase event wire)", async () => {
    const app = createGraph({ name: "stream-updates" })
      .channel("a", { type: "number", default: 0 })
      .channel("b", { type: "number", default: 0 })
      .node("n1", async () => ({ a: 1 }))
      .node("n2", async () => ({ b: 2 }))
      .edge("n1", "n2")
      .compile();

    expect(app.usesRustEngine).toBe(true);
    const events: StreamEvent[] = [];
    for await (const event of app.stream({}, "updates", { runId: "run_stream_upd" as RunId })) {
      events.push(event);
    }
    const updates = events.filter((event) => event.type === "state_update");
    expect(updates.length).toBeGreaterThanOrEqual(2);
    // nodeId is defined — the RunEvent fields now serialize camelCase across napi.
    expect(updates.every((event) => typeof event.nodeId === "string" && event.nodeId.length > 0)).toBe(
      true
    );
    expect(updates.some((event) => event.nodeId === "n1")).toBe(true);
    expect(updates.some((event) => event.nodeId === "n2")).toBe(true);
  });

  it("streams debug events for the full lifecycle", async () => {
    const app = createGraph({ name: "stream-debug" })
      .channel("a", { type: "number", default: 0 })
      .node("n1", async () => ({ a: 1 }))
      .compile();

    expect(app.usesRustEngine).toBe(true);
    const events: StreamEvent[] = [];
    for await (const event of app.stream({}, "debug", { runId: "run_stream_dbg" as RunId })) {
      events.push(event);
    }
    expect(events.every((event) => event.type === "debug")).toBe(true);
    expect(events.length).toBeGreaterThan(0);
  });
});

describe("@adriane-ai/graph-sdk — subgraphs (TS engine, parity)", () => {
  withEngine("ts");

  it("runs a subgraph node on the TS engine via the subgraph resolver", async () => {
    const app = createGraph({ name: "parent-double-ts" })
      .channel("x", { type: "number", default: 5 })
      .channel("y", { type: "number", default: 0 })
      .subgraph("sub", buildDoubler(), {
        inputMapping: { in: "x" },
        outputMapping: { y: "out" }
      })
      .compile();

    expect(app.usesRustEngine).toBe(false);
    const result = await app.run({ x: 5 }, { runId: "run_sub_ts" as RunId });
    expect(result.status).toBe("completed");
    expect(result.channels.y).toBe(10);
  });

  it("propagates a subgraph internal-gate suspension on the TS engine", async () => {
    const app = createGraph({ name: "parent-gated-ts" })
      .channel("result", { type: "string", default: "" })
      .subgraph("sub", buildGatedChild(), { outputMapping: { result: "out" } })
      .compile();

    expect(app.usesRustEngine).toBe(false);
    const suspended = await app.run({}, { runId: "run_sub_gate_ts" as RunId });
    expect(suspended.status).toBe("suspended");

    const resumed = await app.resume(suspended.runId);
    expect(resumed.status).toBe("completed");
    expect(resumed.channels.result).toBe("published");
  });
});
