import { GraphDefinitionSchema } from "./schemas";
import {
  GRAPH_VALIDATION_ERROR_CODES,
  GraphValidationError,
  type GraphValidationErrorCode
} from "./errors";
import type { GraphDefinition } from "./types";

const isNonEmptyCondition = (value: string | undefined): value is string =>
  typeof value === "string" && value.trim().length > 0;

const createError = (
  code: GraphValidationErrorCode,
  message: string,
  path: (string | number)[] = []
): GraphValidationError => new GraphValidationError(code, message, path);

export const validateGraph = (def: GraphDefinition): GraphValidationError[] => {
  const errors: GraphValidationError[] = [];
  const schemaResult = GraphDefinitionSchema.safeParse(def);

  if (!schemaResult.success) {
    for (const issue of schemaResult.error.issues) {
      if (issue.path.includes("condition")) {
        errors.push(
          createError(
            GRAPH_VALIDATION_ERROR_CODES.INVALID_CONDITION_FORMAT,
            issue.message,
            issue.path
          )
        );
      }
    }
  }

  const nodeIds = new Set<string>();
  for (let i = 0; i < def.nodes.length; i += 1) {
    const node = def.nodes[i];
    const nodeId = node?.id;
    if (node === undefined || nodeId === undefined) {
      continue;
    }

    if (nodeIds.has(nodeId)) {
      errors.push(
        createError(
          GRAPH_VALIDATION_ERROR_CODES.DUPLICATE_NODE_ID,
          `Duplicate node id '${nodeId}'.`,
          ["nodes", i, "id"]
        )
      );
      continue;
    }

    nodeIds.add(nodeId);

    if (node.type === "subgraph" && node.subgraphId === undefined) {
      errors.push(
        createError(
          GRAPH_VALIDATION_ERROR_CODES.INVALID_EDGE_REFERENCE,
          `Subgraph node '${nodeId}' must define subgraphId.`,
          ["nodes", i, "subgraphId"]
        )
      );
    }

    const inputMapping = node.inputMapping;
    if (inputMapping !== undefined) {
      const isValidInputMapping = Object.entries(inputMapping).every(
        ([targetKey, sourceKey]) => typeof targetKey === "string" && typeof sourceKey === "string"
      );
      if (!isValidInputMapping) {
        errors.push(
          createError(
            GRAPH_VALIDATION_ERROR_CODES.INVALID_CONDITION_FORMAT,
            `Node '${nodeId}' inputMapping must be string-to-string.`,
            ["nodes", i, "inputMapping"]
          )
        );
      }
    }

    const outputMapping = node.outputMapping;
    if (outputMapping !== undefined) {
      const isValidOutputMapping = Object.entries(outputMapping).every(
        ([targetKey, sourceKey]) => typeof targetKey === "string" && typeof sourceKey === "string"
      );
      if (!isValidOutputMapping) {
        errors.push(
          createError(
            GRAPH_VALIDATION_ERROR_CODES.INVALID_CONDITION_FORMAT,
            `Node '${nodeId}' outputMapping must be string-to-string.`,
            ["nodes", i, "outputMapping"]
          )
        );
      }
    }
  }

  const edgeIds = new Set<string>();
  for (let i = 0; i < def.edges.length; i += 1) {
    const edge = def.edges[i];
    if (edge === undefined) {
      continue;
    }

    if (edgeIds.has(edge.id)) {
      errors.push(
        createError(
          GRAPH_VALIDATION_ERROR_CODES.DUPLICATE_EDGE_ID,
          `Duplicate edge id '${edge.id}'.`,
          ["edges", i, "id"]
        )
      );
    } else {
      edgeIds.add(edge.id);
    }

    if (!nodeIds.has(edge.from)) {
      errors.push(
        createError(
          GRAPH_VALIDATION_ERROR_CODES.INVALID_EDGE_REFERENCE,
          `Edge '${edge.id}' references unknown source node '${edge.from}'.`,
          ["edges", i, "from"]
        )
      );
    }

    if (!nodeIds.has(edge.to)) {
      errors.push(
        createError(
          GRAPH_VALIDATION_ERROR_CODES.INVALID_EDGE_REFERENCE,
          `Edge '${edge.id}' references unknown target node '${edge.to}'.`,
          ["edges", i, "to"]
        )
      );
    }

    if (edge.type === "conditional" && !isNonEmptyCondition(edge.condition)) {
      errors.push(
        createError(
          GRAPH_VALIDATION_ERROR_CODES.INVALID_CONDITION_FORMAT,
          `Conditional edge '${edge.id}' must declare a non-empty named condition.`,
          ["edges", i, "condition"]
        )
      );
    }

    if (edge.condition !== undefined && !isNonEmptyCondition(edge.condition)) {
      errors.push(
        createError(
          GRAPH_VALIDATION_ERROR_CODES.INVALID_CONDITION_FORMAT,
          `Edge '${edge.id}' has an invalid condition format.`,
          ["edges", i, "condition"]
        )
      );
    }
  }

  if (!nodeIds.has(def.entryNodeId)) {
    errors.push(
      createError(
        GRAPH_VALIDATION_ERROR_CODES.MISSING_ENTRY_NODE,
        `Entry node '${def.entryNodeId}' does not exist in nodes.`,
        ["entryNodeId"]
      )
    );
  }

  for (let i = 0; i < def.nodes.length; i += 1) {
    const node = def.nodes[i];
    if (node?.fanOut === undefined) {
      continue;
    }
    for (const fanNodeId of node.fanOut.parallelTo) {
      if (!nodeIds.has(fanNodeId)) {
        errors.push(
          createError(
            GRAPH_VALIDATION_ERROR_CODES.INVALID_EDGE_REFERENCE,
            `Fan-out node '${node.id}' references unknown parallel node '${fanNodeId}'.`,
            ["nodes", i, "fanOut", "parallelTo"]
          )
        );
      }
    }
    if (!nodeIds.has(node.fanOut.joinAt)) {
      errors.push(
        createError(
          GRAPH_VALIDATION_ERROR_CODES.INVALID_EDGE_REFERENCE,
          `Fan-out node '${node.id}' references unknown join node '${node.fanOut.joinAt}'.`,
          ["nodes", i, "fanOut", "joinAt"]
        )
      );
    }
  }

  const hasCycle = (() => {
    const adjacency = new Map<string, string[]>();
    for (const nodeId of nodeIds) {
      adjacency.set(nodeId, []);
    }
    for (const edge of def.edges) {
      const from = adjacency.get(edge.from);
      if (from !== undefined) {
        from.push(edge.to);
      }
    }
    const visiting = new Set<string>();
    const visited = new Set<string>();
    const dfs = (nodeId: string): boolean => {
      if (visiting.has(nodeId)) {
        return true;
      }
      if (visited.has(nodeId)) {
        return false;
      }
      visiting.add(nodeId);
      for (const to of adjacency.get(nodeId) ?? []) {
        if (dfs(to)) {
          return true;
        }
      }
      visiting.delete(nodeId);
      visited.add(nodeId);
      return false;
    };
    for (const nodeId of nodeIds) {
      if (dfs(nodeId)) {
        return true;
      }
    }
    return false;
  })();

  if (hasCycle && def.recursionLimit === undefined) {
    errors.push(
      createError(
        GRAPH_VALIDATION_ERROR_CODES.CYCLE_DETECTED,
        "Cycle detected and recursionLimit is missing. Set recursionLimit to acknowledge cyclic execution.",
        ["recursionLimit"]
      )
    );
  }

  return errors;
};
