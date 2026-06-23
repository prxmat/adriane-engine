import type { ToolDefinition, ToolHandler, ToolId } from "./tools.js";

/**
 * The `writeTodos` planning tool and its checkpointed state shape — phase 1 of the
 * governed deep-agent harness (ADR 0022/0023). Behaviour-identical to the Rust
 * `agents-core` `todos.rs`: a pure state-write tool the model calls to (re)emit the
 * **authoritative full** todo list. The engine (Rust) executes it and the agent
 * node persists the latest list into {@link TODOS_CHANNEL}.
 *
 * This module is the shared-shape source of truth (types + {@link normalizeTodos})
 * plus the tool definition SDK consumers reference; the real execution path is the
 * Rust engine (the SDK runs Rust-only — ADR 0016).
 */

export type TodoStatus = "pending" | "in_progress" | "completed";

export type TodoItem = { id: string; text: string; status: TodoStatus };

/**
 * Reserved channel the agent node persists the latest todo list into. Replace
 * semantics: `writeTodos` always writes the full authoritative list, so the channel
 * must not declare an append/merge reducer. Matches the Rust `TODOS_CHANNEL`.
 */
export const TODOS_CHANNEL = "__todos";

/** The `writeTodos` tool name. Matches the Rust `WRITE_TODOS_TOOL`. */
export const WRITE_TODOS_TOOL_NAME = "writeTodos";

export type WriteTodosInput = { todos: Array<{ id?: string; text: string; status: TodoStatus }> };

/**
 * Normalize raw tool input into an authoritative todo list. Lenient and
 * deterministic — **byte-for-byte parity** with the Rust `normalize_todos`:
 * - iterate `input.todos` in order;
 * - drop any item whose `text` is missing or blank (after trimming);
 * - an item with a missing or blank `id` gets `todo-{n}`, where `n` is its
 *   **1-based position in the incoming list** (dropped items still advance `n`);
 * - an unknown or absent `status` coerces to `pending`.
 *
 * A missing / non-array `todos` field yields an empty list.
 */
export function normalizeTodos(input: unknown): TodoItem[] {
  const todos = (input as { todos?: unknown } | null | undefined)?.todos;
  if (!Array.isArray(todos)) {
    return [];
  }
  const out: TodoItem[] = [];
  todos.forEach((raw, index) => {
    const row = (raw ?? {}) as Record<string, unknown>;
    const text = typeof row.text === "string" ? row.text.trim() : "";
    if (text === "") {
      return;
    }
    const rawId = typeof row.id === "string" ? row.id : "";
    const id = rawId.trim() !== "" ? rawId : `todo-${index + 1}`;
    const status: TodoStatus =
      row.status === "in_progress" ? "in_progress" : row.status === "completed" ? "completed" : "pending";
    out.push({ id, text, status });
  });
  return out;
}

/** JSON Schema advertised to the LLM — identical to the Rust `write_todos_tool` schema. */
export const writeTodosJsonSchema: Record<string, unknown> = {
  type: "object",
  properties: {
    todos: {
      type: "array",
      items: {
        type: "object",
        properties: {
          id: { type: "string" },
          text: { type: "string" },
          status: { type: "string", enum: ["pending", "in_progress", "completed"] }
        },
        required: ["text", "status"]
      }
    }
  },
  required: ["todos"],
  additionalProperties: false
};

/**
 * The `writeTodos` tool definition + handler. `requiresApproval` is always false —
 * planning is cheap and never gated. The handler returns the normalized list.
 */
export const writeTodosTool: {
  definition: ToolDefinition<WriteTodosInput, TodoItem[]>;
  handler: ToolHandler<WriteTodosInput, TodoItem[]>;
} = {
  definition: {
    id: WRITE_TODOS_TOOL_NAME as unknown as ToolId,
    name: WRITE_TODOS_TOOL_NAME,
    description:
      "Record or update your plan as a todo list. Always re-emit the COMPLETE authoritative list " +
      "(every call replaces the previous one). Each item: a short `text` and a `status` of " +
      "pending, in_progress, or completed.",
    inputSchema: { parse: (input: unknown) => input as WriteTodosInput },
    outputSchema: { parse: (input: unknown) => input as TodoItem[] },
    permissions: [],
    requiresApproval: false,
    jsonSchema: writeTodosJsonSchema
  },
  handler: async (input: WriteTodosInput) => normalizeTodos(input)
};
