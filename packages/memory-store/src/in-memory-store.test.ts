import { describe, expect, it } from "vitest";

import { InMemoryStore } from "./in-memory-store.js";

describe("InMemoryStore", () => {
  it("supports put get and delete", async () => {
    const store = new InMemoryStore();
    await store.put(["user:u1"], "profile", { name: "Ada" });
    const item = await store.get(["user:u1"], "profile");
    expect(item?.value).toEqual({ name: "Ada" });
    await store.delete(["user:u1"], "profile");
    const afterDelete = await store.get(["user:u1"], "profile");
    expect(afterDelete).toBeUndefined();
  });

  it("lists keys with prefix", async () => {
    const store = new InMemoryStore();
    await store.put(["agent:risk"], "memo:1", { risk: "high" });
    await store.put(["agent:risk"], "memo:2", { risk: "low" });
    await store.put(["agent:risk"], "note:1", { note: true });
    const list = await store.list(["agent:risk"], "memo:");
    expect(list.map((item) => item.key)).toEqual(["memo:1", "memo:2"]);
  });

  it("performs basic textual search", async () => {
    const store = new InMemoryStore();
    await store.put(["agent:risk"], "memo:1", { summary: "critical supplier risk" });
    await store.put(["agent:risk"], "memo:2", { summary: "stable account" });
    const found = await store.search(["agent:risk"], "critical", 5);
    expect(found).toHaveLength(1);
    expect(found[0]?.key).toBe("memo:1");
  });
});
