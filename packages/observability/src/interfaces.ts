import type { ObservabilityEvent, Metric, Span, SpanId, SpanStatus } from "./types.js";

export interface Tracer {
  startSpan(name: string, attrs?: Record<string, unknown>, parentSpanId?: SpanId): Span;
  endSpan(spanId: SpanId, status: SpanStatus, error?: string): void;
  getSpan(spanId: SpanId): Span | undefined;
  getTrace(traceId: Span["traceId"]): Span[];
}

export interface MetricCollector {
  record(metric: Omit<Metric, "timestamp">): void;
  query(name: string, tags?: Record<string, string>): Metric[];
}

export interface ObservabilityBus {
  emit(event: ObservabilityEvent): void;
  subscribe(handler: (event: ObservabilityEvent) => void): () => void;
}
