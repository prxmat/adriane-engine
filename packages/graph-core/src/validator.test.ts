import { describe, expect, it } from "vitest";

import { GRAPH_VALIDATION_ERROR_CODES } from "./errors.js";
import { validateGraph } from "./validator.js";
import type { GraphDefinition } from "./types.js";

const createValidGraph = (): GraphDefinition => ({
  id: "graph-1" as GraphDefinition["id"],
  version: "1.0.0",
  name: "Valid Graph",
  channels: {
    result: { type: "object", reducer: "merge", default: {} }
  },
  entryNodeId: "node-1" as GraphDefinition["entryNodeId"],
  nodes: [
    {
      id: "node-1" as GraphDefinition["nodes"][number]["id"],
      type: "action",
      label: "Start"
    },
    {
      id: "node-2" as GraphDefinition["nodes"][number]["id"],
      type: "tool",
      label: "Continue"
    }
  ],
  edges: [
    {
      id: "edge-1" as GraphDefinition["edges"][number]["id"],
      from: "node-1" as GraphDefinition["nodes"][number]["id"],
      to: "node-2" as GraphDefinition["nodes"][number]["id"],
      type: "default"
    }
  ]
});

describe("validateGraph", () => {
  it("returns no errors for a valid graph definition", () => {
    const graph = createValidGraph();

    const errors = validateGraph(graph);

    expect(errors).toHaveLength(0);
  });

  it("returns duplicate node id error", () => {
    const graph = createValidGraph();
    graph.nodes.push({
      id: "node-1" as GraphDefinition["nodes"][number]["id"],
      type: "agent",
      label: "Duplicate Node"
    });

    const errors = validateGraph(graph);

    expect(errors.some((error) => error.code === GRAPH_VALIDATION_ERROR_CODES.DUPLICATE_NODE_ID)).toBe(
      true
    );
  });

  it("returns missing entry node error", () => {
    const graph = createValidGraph();
    graph.entryNodeId = "missing-entry" as GraphDefinition["entryNodeId"];

    const errors = validateGraph(graph);

    expect(errors.some((error) => error.code === GRAPH_VALIDATION_ERROR_CODES.MISSING_ENTRY_NODE)).toBe(
      true
    );
  });

  it("returns invalid edge reference error", () => {
    const graph = createValidGraph();
    const firstEdge = graph.edges[0];
    if (firstEdge === undefined) {
      throw new Error("Expected first edge to exist.");
    }

    graph.edges[0] = {
      ...firstEdge,
      to: "missing-node" as GraphDefinition["nodes"][number]["id"]
    };

    const errors = validateGraph(graph);

    expect(
      errors.some((error) => error.code === GRAPH_VALIDATION_ERROR_CODES.INVALID_EDGE_REFERENCE)
    ).toBe(true);
  });

  it("returns invalid condition format error", () => {
    const graph = createValidGraph();
    const firstEdge = graph.edges[0];
    if (firstEdge === undefined) {
      throw new Error("Expected first edge to exist.");
    }

    graph.edges[0] = {
      ...firstEdge,
      type: "conditional",
      condition: "   "
    };

    const errors = validateGraph(graph);

    expect(
      errors.some((error) => error.code === GRAPH_VALIDATION_ERROR_CODES.INVALID_CONDITION_FORMAT)
    ).toBe(true);
  });

  it("returns error when subgraph node misses subgraphId", () => {
    const graph = createValidGraph();
    const firstNode = graph.nodes[0];
    if (firstNode === undefined) {
      throw new Error("Expected first node to exist in valid graph fixture.");
    }
    graph.nodes[0] = {
      ...firstNode,
      type: "subgraph"
    };

    const errors = validateGraph(graph);

    expect(errors.some((error) => error.path.join(".") === "nodes.0.subgraphId")).toBe(true);
  });

  it("returns cycle warning error when recursionLimit missing", () => {
    const graph = createValidGraph();
    graph.edges.push({
      id: "edge-2" as GraphDefinition["edges"][number]["id"],
      from: "node-2" as GraphDefinition["nodes"][number]["id"],
      to: "node-1" as GraphDefinition["nodes"][number]["id"],
      type: "default"
    });

    const errors = validateGraph(graph);
    expect(errors.some((error) => error.code === GRAPH_VALIDATION_ERROR_CODES.CYCLE_DETECTED)).toBe(
      true
    );
  });

  it("accepts cycle when recursionLimit is defined", () => {
    const graph = createValidGraph();
    graph.recursionLimit = 5;
    graph.edges.push({
      id: "edge-2" as GraphDefinition["edges"][number]["id"],
      from: "node-2" as GraphDefinition["nodes"][number]["id"],
      to: "node-1" as GraphDefinition["nodes"][number]["id"],
      type: "default"
    });

    const errors = validateGraph(graph);
    expect(errors.some((error) => error.code === GRAPH_VALIDATION_ERROR_CODES.CYCLE_DETECTED)).toBe(
      false
    );
  });
});
