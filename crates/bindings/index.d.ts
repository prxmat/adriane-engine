/** Version of the bound Rust engine. */
export function engineVersion(): string;

/**
 * Validate a graph definition (JSON). Returns a JSON array of validation errors
 * (`[]` when sound). Throws on malformed JSON.
 */
export function validateGraphJson(definitionJson: string): string;

/**
 * Compile graph DSL YAML into a validated `GraphDefinition` (JSON string).
 * Throws with a clear message on parse, DSL, or structural validation failure.
 */
export function compileGraphYamlJson(yaml: string): string;

/**
 * A JS callback invoked from Rust during a run. Receives a JSON string payload.
 *
 * `onNode` and `onCondition` may be **async**: they can return a `Promise`, and Rust
 * awaits it (the napi bridge resolves the returned promise to its JS-resolved value
 * before continuing). A synchronous return is still accepted. `onEvent` is
 * fire-and-forget — its return is never awaited.
 *
 * - `onNode(payloadJson)`: `payloadJson` is either
 *   `{ kind: "node", nodeId, input, state }` (a custom JS node handler — return the
 *   channel-update JSON, or a `Promise` resolving to it) or
 *   `{ kind: "tool", name, input }` (a JS tool `execute` fn — return the tool-result
 *   JSON, or a `Promise` resolving to it).
 * - `onCondition(payloadJson)`: `payloadJson` is `{ name, state }`; return a boolean,
 *   a boolean-ish string (`"true"`/`"false"`), or a `Promise` resolving to either.
 * - `onEvent(payloadJson)`: `payloadJson` is a serialized `RunEvent`; return nothing
 *   (fire-and-forget).
 */
export type EngineNodeCallback = (payloadJson: string) => string | Promise<string>;
export type EngineConditionCallback = (
  payloadJson: string
) => boolean | string | Promise<boolean | string>;
export type EngineEventCallback = (payloadJson: string) => void;

/**
 * Start a fresh run of a graph on the Rust engine. `specJson` is the `EngineSpec`
 * (graph, runId, initialData, agents, jsNodeIds, jsToolNames). Resolves to a JSON
 * `RunOutcome` (`{ state, status, pendingApprovals }`).
 */
export function engineRun(
  specJson: string,
  onNode: EngineNodeCallback,
  onCondition: EngineConditionCallback,
  onEvent: EngineEventCallback
): Promise<string>;

/**
 * Resume a previously suspended run from its serialized state (`specJson.state`).
 * Resolves to a JSON `RunOutcome`.
 */
export function engineResume(
  specJson: string,
  onNode: EngineNodeCallback,
  onCondition: EngineConditionCallback,
  onEvent: EngineEventCallback
): Promise<string>;

/**
 * Grant the tools in `specJson.approvedTools` (written into the `__approvedTools`
 * channel) and resume. Resolves to a JSON `RunOutcome`.
 */
export function engineApproveAndResume(
  specJson: string,
  onNode: EngineNodeCallback,
  onCondition: EngineConditionCallback,
  onEvent: EngineEventCallback
): Promise<string>;
