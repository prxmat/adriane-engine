import { describe, expect, it } from "vitest";

import { InMemoryToolRegistry } from "./tools.js";
import type { ToolId } from "./tools.js";

describe("InMemoryToolRegistry", () => {
  it("registers resolves and lists tools", async () => {
    const registry = new InMemoryToolRegistry();
    const definition = {
      id: "tool-1" as ToolId,
      name: "echo",
      description: "Echo tool",
      inputSchema: { parse: (input: unknown) => input as { text: string } },
      outputSchema: { parse: (output: unknown) => output as { text: string } },
      permissions: ["read"]
    };

    registry.register(definition, async (input) => ({ text: (input as { text: string }).text }));

    const resolved = registry.resolve("tool-1" as ToolId);
    expect(resolved?.definition.name).toBe("echo");
    expect(registry.list()).toHaveLength(1);
    const output = await resolved!.handler({ text: "hello" });
    expect(output).toEqual({ text: "hello" });
  });
});
