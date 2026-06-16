import { describe, expect, it } from "vitest";

import { InMemoryMetricCollector } from "./in-memory-metric-collector.js";

describe("InMemoryMetricCollector", () => {
  it("records metrics with timestamp", () => {
    const collector = new InMemoryMetricCollector();

    collector.record({
      name: "runtime.duration",
      value: 120,
      unit: "ms",
      tags: { runId: "run-1" }
    });

    const metrics = collector.query("runtime.duration");
    expect(metrics).toHaveLength(1);
    expect(metrics[0]?.timestamp).toBeInstanceOf(Date);
  });

  it("queries by name and tags", () => {
    const collector = new InMemoryMetricCollector();
    collector.record({
      name: "runtime.duration",
      value: 120,
      unit: "ms",
      tags: { runId: "run-1", nodeId: "A" }
    });
    collector.record({
      name: "runtime.duration",
      value: 220,
      unit: "ms",
      tags: { runId: "run-2", nodeId: "B" }
    });
    collector.record({
      name: "tokens.prompt",
      value: 42,
      unit: "count",
      tags: { runId: "run-1" }
    });

    const run1Duration = collector.query("runtime.duration", { runId: "run-1" });
    const run2Duration = collector.query("runtime.duration", { runId: "run-2" });
    const allDuration = collector.query("runtime.duration");

    expect(run1Duration).toHaveLength(1);
    expect(run2Duration).toHaveLength(1);
    expect(allDuration).toHaveLength(2);
  });
});
