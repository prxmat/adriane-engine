import { describe, expect, it } from "vitest";

import { normalizeTodos, writeTodosTool, TODOS_CHANNEL, WRITE_TODOS_TOOL_NAME } from "./todos.js";

// Behaviour parity with the Rust `agents-core` `todos.rs` tests — the SAME fixtures
// must produce the SAME ids / statuses / order on both engines (the one-engine
// invariant; the SDK runs Rust-only but these shapes must agree).

describe("normalizeTodos", () => {
  it("mints 1-based ids for missing or blank ids", () => {
    const todos = normalizeTodos({
      todos: [
        { text: "first", status: "pending" },
        { id: "  ", text: "second", status: "in_progress" },
        { id: "keep", text: "third", status: "completed" }
      ]
    });
    expect(todos).toHaveLength(3);
    expect(todos[0]?.id).toBe("todo-1");
    expect(todos[1]?.id).toBe("todo-2");
    expect(todos[2]?.id).toBe("keep");
  });

  it("drops blank-text rows but keeps the 1-based incoming position", () => {
    const todos = normalizeTodos({
      todos: [
        { text: "first", status: "pending" },
        { text: "   ", status: "pending" },
        { text: "third", status: "pending" }
      ]
    });
    expect(todos).toHaveLength(2);
    expect(todos[0]?.id).toBe("todo-1");
    expect(todos[1]?.id).toBe("todo-3");
  });

  it("coerces an unknown or absent status to pending", () => {
    const todos = normalizeTodos({
      todos: [
        { text: "a", status: "bogus" },
        { text: "b" }
      ]
    });
    expect(todos[0]?.status).toBe("pending");
    expect(todos[1]?.status).toBe("pending");
  });

  it("yields an empty list for a missing or non-array todos field", () => {
    expect(normalizeTodos({})).toEqual([]);
    expect(normalizeTodos({ todos: "nope" })).toEqual([]);
    expect(normalizeTodos(null)).toEqual([]);
    expect(normalizeTodos(undefined)).toEqual([]);
  });
});

describe("writeTodosTool", () => {
  it("is named writeTodos, is never gated, and advertises a JSON schema", () => {
    expect(writeTodosTool.definition.name).toBe(WRITE_TODOS_TOOL_NAME);
    expect(writeTodosTool.definition.requiresApproval).toBe(false);
    expect(writeTodosTool.definition.jsonSchema).toBeDefined();
    expect(writeTodosTool.definition.permissions).toEqual([]);
  });

  it("normalizes its input through the handler", async () => {
    const out = await writeTodosTool.handler({
      todos: [{ text: "do thing", status: "pending" }]
    });
    expect(out).toHaveLength(1);
    expect(out[0]?.id).toBe("todo-1");
    expect(out[0]?.status).toBe("pending");
  });

  it("exposes the reserved durable channel name", () => {
    expect(TODOS_CHANNEL).toBe("__todos");
  });
});
