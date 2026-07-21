import { describe, expect, it } from "vitest";

import {
  resumeCatalogGraph,
  runCatalogGraph,
  rustEngineAvailable,
  type GraphDefinition,
  type RunId
} from "./index.js";

/**
 * ADR 0042 D1 (product ADR 0068 — child workflows) — subgraph nodes on the CATALOG run path.
 * `RunCatalogGraphOptions.subgraphs` threads child `GraphDefinition`s into `EngineSpec.subgraphs`,
 * the exact wire field the builder path (`CompiledGraph`, `subgraph.test.ts`) already exercises —
 * this proves the SAME `execute_subgraph` mechanism resumes/suspends/completes correctly when
 * reached from a plain catalog `GraphDefinition` (no `GraphBuilder`), the path the product uses.
 */
// The catalog path has no TS handler closures — a plain `action` node is an inert no-op stub
// there (unlike the builder path's `.node("calc", async (...) => ...)`, which runs real TS). So
// this child can't COMPUTE a transform; instead its `out` channel default is a value the parent
// could never produce on its own, proving the subgraph ran and its outputMapping wired correctly.
const doublerChild: GraphDefinition = {
  id: "sub-doubler",
  version: "1",
  name: "sub-doubler",
  channels: {
    in: { type: "number", reducer: "replace", default: 0 },
    out: { type: "number", reducer: "replace", default: 4200 }
  },
  nodes: [{ id: "calc", type: "action", label: "calc" }],
  edges: [],
  entryNodeId: "calc"
} as unknown as GraphDefinition;

const gatedChild: GraphDefinition = {
  id: "sub-gated",
  version: "1",
  name: "sub-gated",
  channels: {
    drafted: { type: "boolean", reducer: "replace", default: false },
    out: { type: "string", reducer: "replace", default: "" }
  },
  nodes: [
    { id: "c_draft", type: "action", label: "c_draft" },
    { id: "c_gate", type: "human-gate", label: "c_gate" },
    { id: "c_publish", type: "action", label: "c_publish" }
  ],
  edges: [
    { id: "e1", from: "c_draft", to: "c_gate", type: "default" },
    { id: "e2", from: "c_gate", to: "c_publish", type: "default" }
  ],
  entryNodeId: "c_draft"
} as unknown as GraphDefinition;

const parentWithChild = (childId: string, outputMapping?: Record<string, string>): GraphDefinition =>
  ({
    id: "parent-with-subgraph",
    version: "1",
    name: "parent-with-subgraph",
    channels: {
      x: { type: "number", reducer: "replace", default: 21 },
      y: { type: "number", reducer: "replace", default: 0 }
    },
    nodes: [
      {
        id: "sub",
        type: "subgraph",
        label: "sub",
        subgraphId: childId,
        inputMapping: { in: "x" },
        outputMapping: outputMapping ?? { y: "out" }
      }
    ],
    edges: [],
    entryNodeId: "sub"
  }) as unknown as GraphDefinition;

describe("@adriane-ai/graph-sdk — catalog subgraphs (ADR 0042 D1)", () => {
  (rustEngineAvailable() ? it : it.skip)(
    "runs a subgraph node from a plain catalog GraphDefinition, mapping channels in and out",
    async () => {
      const outcome = await runCatalogGraph(parentWithChild("sub-doubler"), {
        runId: "run_catalog_sub_double" as RunId,
        initialData: { x: 21 },
        subgraphs: [doublerChild]
      });
      expect(outcome.status).toBe("completed");
      expect(outcome.state.channels.y).toBe(4200);
    }
  );

  (rustEngineAvailable() ? it : it.skip)(
    "suspends when a catalog subgraph hits an internal human gate, then resumes via resumeCatalogGraph",
    async () => {
      const definition = parentWithChild("sub-gated", { y: "out" });
      const suspended = await runCatalogGraph(definition, {
        runId: "run_catalog_sub_gate" as RunId,
        initialData: {},
        subgraphs: [gatedChild]
      });
      expect(suspended.status).toBe("suspended");

      const resumed = await resumeCatalogGraph(definition, suspended.state, {
        subgraphs: [gatedChild]
      });
      expect(resumed.status).toBe("completed");
    }
  );

  (rustEngineAvailable() ? it : it.skip)(
    "runs exactly as before (no subgraph nodes) when subgraphs is omitted",
    async () => {
      const plain: GraphDefinition = {
        id: "plain-no-subgraph",
        version: "1",
        name: "plain-no-subgraph",
        channels: { a: { type: "number", reducer: "replace", default: 0 } },
        nodes: [{ id: "n1", type: "action", label: "n1" }],
        edges: [],
        entryNodeId: "n1"
      } as unknown as GraphDefinition;
      const outcome = await runCatalogGraph(plain, { runId: "run_catalog_no_sub" as RunId });
      expect(outcome.status).toBe("completed");
    }
  );
});
