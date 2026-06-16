import { describe, expect, it } from "vitest";

import { InMemoryTracer } from "./in-memory-tracer.js";

describe("InMemoryTracer", () => {
  it("start/end span updates lifecycle fields", () => {
    const tracer = new InMemoryTracer();
    const span = tracer.startSpan("graph.run", { phase: "start" });

    tracer.endSpan(span.id, "ok");

    const ended = tracer.getSpan(span.id);
    expect(ended).toBeDefined();
    expect(ended?.status).toBe("ok");
    expect(ended?.endedAt).toBeInstanceOf(Date);
  });

  it("supports parent-child spans", () => {
    const tracer = new InMemoryTracer();
    const parent = tracer.startSpan("parent");
    const child = tracer.startSpan("child", { nested: true }, parent.id);

    expect(child.parentSpanId).toBe(parent.id);
    expect(child.traceId).toBe(parent.traceId);
  });

  it("getTrace returns all spans in same trace", () => {
    const tracer = new InMemoryTracer();
    const root = tracer.startSpan("root");
    const child1 = tracer.startSpan("child1", undefined, root.id);
    const child2 = tracer.startSpan("child2", undefined, root.id);
    tracer.startSpan("other-trace");

    const traceSpans = tracer.getTrace(root.traceId);
    const ids = traceSpans.map((span) => span.id);

    expect(ids).toContain(root.id);
    expect(ids).toContain(child1.id);
    expect(ids).toContain(child2.id);
    expect(traceSpans).toHaveLength(3);
  });
});
