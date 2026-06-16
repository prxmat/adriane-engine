import { describe, expect, it } from "vitest";

import type { Message } from "./messages.js";
import { filterMessages, trimMessages } from "./types.js";

const createMessage = (role: Message["role"], content: string): Message => {
  const id = `${role}-${content}` as Message["id"];
  const createdAt = new Date();
  switch (role) {
    case "human":
      return { id, role, content, createdAt };
    case "ai":
      return { id, role, content, createdAt };
    case "tool":
      return { id, role, toolCallId: "tool-call-1", content, createdAt };
    case "system":
      return { id, role, content, createdAt };
  }
};

describe("messages helpers", () => {
  it("trimMessages removes oldest messages first", () => {
    const messages: Message[] = [
      createMessage("system", "s"),
      createMessage("human", "h"),
      createMessage("ai", "a")
    ];
    const trimmed = trimMessages(messages, 2, () => 1);
    expect(trimmed.map((m) => m.role)).toEqual(["human", "ai"]);
  });

  it("filterMessages keeps only requested roles", () => {
    const messages: Message[] = [
      createMessage("system", "s"),
      createMessage("human", "h"),
      createMessage("ai", "a"),
      createMessage("tool", "t")
    ];
    const filtered = filterMessages(messages, ["human", "ai"]);
    expect(filtered.map((m) => m.role)).toEqual(["human", "ai"]);
  });
});
