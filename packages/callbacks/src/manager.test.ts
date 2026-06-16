import { describe, expect, it } from "vitest";

import { InMemoryCallbackManager } from "./manager.js";
import type { CallbackHandler } from "./interfaces.js";

describe("InMemoryCallbackManager", () => {
  it("emit calls matching handler method", async () => {
    const calls: string[] = [];
    const handler: CallbackHandler = {
      onNodeStart: async () => {
        calls.push("onNodeStart");
      }
    };
    const manager = new InMemoryCallbackManager([handler]);
    await manager.emit({
      type: "onNodeStart",
      runId: "run-1",
      timestamp: new Date().toISOString(),
      input: {}
    });
    expect(calls).toEqual(["onNodeStart"]);
  });

  it("child inherits handlers and adds tags/metadata", async () => {
    const seenTags: string[][] = [];
    const seenMetadata: Array<Record<string, unknown> | undefined> = [];
    const handler: CallbackHandler = {
      onChainStart: async (event) => {
        seenTags.push(event.tags ?? []);
        seenMetadata.push(event.metadata);
      }
    };
    const root = new InMemoryCallbackManager([handler], ["root"], { scope: "global" });
    const child = root.createChild(["child"], { unit: "test" });

    await child.emit({
      type: "onChainStart",
      runId: "run-2",
      timestamp: new Date().toISOString(),
      input: {}
    });

    expect(seenTags[0]).toEqual(["root", "child"]);
    expect(seenMetadata[0]).toEqual({ scope: "global", unit: "test" });
  });

  it("handler errors do not break emit", async () => {
    const calls: string[] = [];
    const manager = new InMemoryCallbackManager([
      {
        onNodeEnd: async () => {
          throw new Error("boom");
        }
      },
      {
        onNodeEnd: async () => {
          calls.push("second");
        }
      }
    ]);

    await manager.emit({
      type: "onNodeEnd",
      runId: "run-3",
      timestamp: new Date().toISOString(),
      output: {}
    });

    expect(calls).toEqual(["second"]);
  });
});
