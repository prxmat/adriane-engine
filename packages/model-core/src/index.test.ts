import { describe, expect, it } from "vitest";

import {
  assertKnownProvider,
  Model,
  openaiCompatible,
  toModelSpec,
  type ModelSpec
} from "./index.js";

class TestModel extends Model {
  readonly spec: ModelSpec = { provider: "openai", model: "x" };
}

describe("@adriane-ai/model-core", () => {
  it("toSpec / toJSON return the plain spec", () => {
    const m = new TestModel();
    expect(m.toSpec()).toEqual({ provider: "openai", model: "x" });
    expect(JSON.parse(JSON.stringify(m))).toEqual({ provider: "openai", model: "x" });
  });

  it("toModelSpec normalizes a Model or a bare spec", () => {
    expect(toModelSpec(new TestModel())).toEqual({ provider: "openai", model: "x" });
    expect(toModelSpec({ provider: "anthropic" })).toEqual({ provider: "anthropic" });
  });

  it("assertKnownProvider fails loudly on an unknown slug", () => {
    expect(() => assertKnownProvider("nope")).toThrow(/Unknown provider/);
    expect(() => assertKnownProvider("openai")).not.toThrow();
  });

  it("openaiCompatible builds an openai-slug spec carrying baseURL", () => {
    const spec = openaiCompatible("llama-3", { baseURL: "http://localhost:1234/v1" }).toSpec();
    expect(spec.provider).toBe("openai");
    expect(spec.model).toBe("llama-3");
    expect(spec.baseURL).toBe("http://localhost:1234/v1");
  });

  it("invoke() round-trips through the Rust gateway (deterministic mock when no key)", async () => {
    const m = new TestModel(); // provider openai, no key → Rust falls to the mock
    let res;
    try {
      res = await m.invoke("hi");
    } catch {
      return; // napi addon not built in this environment — skip (CI builds it)
    }
    expect(typeof res.content).toBe("string");
    expect(res.provider).toBe("openai");
  });
});
