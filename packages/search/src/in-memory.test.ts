import { describe, expect, it } from "vitest";

import { InMemorySearchProvider } from "./in-memory.js";
import type { SearchDocument } from "./types.js";

const doc = (
  over: Partial<SearchDocument> & Pick<SearchDocument, "type" | "id">
): SearchDocument => ({
  tenantId: "t1",
  title: "",
  text: "",
  href: "/x",
  ...over
});

describe("InMemorySearchProvider", () => {
  it("ranks title matches above body-only matches", async () => {
    const p = new InMemorySearchProvider();
    await p.index([
      doc({ type: "graph", id: "g1", title: "Refund approval flow", text: "handles money" }),
      doc({ type: "graph", id: "g2", title: "Onboarding", text: "a refund step somewhere" })
    ]);

    const hits = await p.search("refund", { tenantId: "t1" });

    expect(hits.map((h) => h.id)).toEqual(["g1", "g2"]);
    expect(hits[0]!.score).toBeGreaterThan(hits[1]!.score);
  });

  it("returns a tenant's own docs plus global (tenantId null) docs, never another tenant's", async () => {
    const p = new InMemorySearchProvider();
    await p.index([
      doc({ type: "graph", id: "mine", tenantId: "t1", title: "alpha graph" }),
      doc({ type: "agent", id: "global", tenantId: null, title: "alpha agent" }),
      doc({ type: "graph", id: "theirs", tenantId: "t2", title: "alpha secret" })
    ]);

    const ids = (await p.search("alpha", { tenantId: "t1" })).map((h) => h.id).sort();

    expect(ids).toEqual(["global", "mine"]);
  });

  it("honours the type filter and the limit", async () => {
    const p = new InMemorySearchProvider();
    await p.index([
      doc({ type: "graph", id: "g", title: "report graph" }),
      doc({ type: "kb", id: "k", title: "report doc" }),
      doc({ type: "agent", id: "a", title: "report agent" })
    ]);

    const onlyKb = await p.search("report", { tenantId: "t1", types: ["kb"] });
    expect(onlyKb.map((h) => h.type)).toEqual(["kb"]);

    const capped = await p.search("report", { tenantId: "t1", limit: 2 });
    expect(capped).toHaveLength(2);
  });

  it("upserts by (type,id) and removes", async () => {
    const p = new InMemorySearchProvider();
    await p.index([doc({ type: "graph", id: "g1", title: "old name" })]);
    await p.index([doc({ type: "graph", id: "g1", title: "new shiny name" })]);

    expect(await p.search("shiny", { tenantId: "t1" })).toHaveLength(1);
    expect(await p.search("old", { tenantId: "t1" })).toHaveLength(0);

    await p.remove("graph", "g1");
    expect(await p.search("shiny", { tenantId: "t1" })).toHaveLength(0);
  });

  it("returns nothing for a blank query", async () => {
    const p = new InMemorySearchProvider();
    await p.index([doc({ type: "graph", id: "g1", title: "anything" })]);
    expect(await p.search("   ", { tenantId: "t1" })).toEqual([]);
  });
});
