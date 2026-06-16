export const GRAPH_VALIDATION_ERROR_CODES = {
  DUPLICATE_NODE_ID: "DUPLICATE_NODE_ID",
  DUPLICATE_EDGE_ID: "DUPLICATE_EDGE_ID",
  MISSING_ENTRY_NODE: "MISSING_ENTRY_NODE",
  INVALID_EDGE_REFERENCE: "INVALID_EDGE_REFERENCE",
  CYCLE_DETECTED: "CYCLE_DETECTED",
  INVALID_CONDITION_FORMAT: "INVALID_CONDITION_FORMAT"
} as const;

export type GraphValidationErrorCode =
  (typeof GRAPH_VALIDATION_ERROR_CODES)[keyof typeof GRAPH_VALIDATION_ERROR_CODES];

export type GraphValidationPath = (string | number)[];

export class GraphValidationError extends Error {
  public readonly code: GraphValidationErrorCode;
  public readonly path: GraphValidationPath;

  public constructor(
    code: GraphValidationErrorCode,
    message: string,
    path: GraphValidationPath = []
  ) {
    super(message);
    this.name = "GraphValidationError";
    this.code = code;
    this.path = path;
  }
}
