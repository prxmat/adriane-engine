import type { MetricCollector } from "./interfaces.js";
import type { Metric } from "./types.js";

const hasTags = (metricTags: Record<string, string>, queryTags: Record<string, string>): boolean =>
  Object.entries(queryTags).every(([key, value]) => metricTags[key] === value);

export class InMemoryMetricCollector implements MetricCollector {
  private readonly metrics: Metric[] = [];

  public record(metric: Omit<Metric, "timestamp">): void {
    this.metrics.push({
      ...metric,
      timestamp: new Date()
    });
  }

  public query(name: string, tags?: Record<string, string>): Metric[] {
    return this.metrics.filter((metric) => {
      if (metric.name !== name) {
        return false;
      }
      if (tags === undefined) {
        return true;
      }
      return hasTags(metric.tags, tags);
    });
  }
}
