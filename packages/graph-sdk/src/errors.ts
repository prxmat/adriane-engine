import type { GraphValidationError } from "@adriane-ai/graph-core";

/**
 * Discriminated-union result type used across the SDK's "safe" entry points
 * (e.g. {@link GraphBuilder.safeCompile}). Mirrors Zod's `safeParse` ergonomics.
 */
export type Result<T, E> = { success: true; data: T } | { success: false; error: E };

/** Base class for every error thrown by `@adriane-ai/graph-sdk`. */
export class AdrianeSdkError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = "AdrianeSdkError";
  }
}

/** Thrown when `.compile()` is called on a graph that fails validation. */
export class GraphCompileError extends AdrianeSdkError {
  public readonly errors: GraphValidationError[];

  public constructor(errors: GraphValidationError[]) {
    const summary = errors.map((error) => `${error.code}: ${error.message}`).join("; ");
    super(`Graph failed to compile: ${summary}`);
    this.name = "GraphCompileError";
    this.errors = errors;
  }
}

/** Thrown when two nodes are added under the same id. */
export class DuplicateNodeError extends AdrianeSdkError {
  public constructor(nodeId: string) {
    super(`A node with id '${nodeId}' was already added to this graph.`);
    this.name = "DuplicateNodeError";
  }
}

/** Thrown when an action node is added without an executable handler. */
export class MissingHandlerError extends AdrianeSdkError {
  public constructor(nodeId: string) {
    super(`Node '${nodeId}' is an action node but no handler was provided.`);
    this.name = "MissingHandlerError";
  }
}

/** Thrown when a builder method references a node id that has not been added yet. */
export class UnknownNodeError extends AdrianeSdkError {
  public constructor(nodeId: string, context: string) {
    super(`${context} references node '${nodeId}', which has not been added to this graph.`);
    this.name = "UnknownNodeError";
  }
}
