import type { Tracer } from "./interfaces.js";
import type { Span, SpanId, SpanStatus, TraceId } from "./types.js";

const createSpanId = (): SpanId =>
  `span-${Date.now()}-${Math.random().toString(36).slice(2, 8)}` as SpanId;
const createTraceId = (): TraceId =>
  `trace-${Date.now()}-${Math.random().toString(36).slice(2, 8)}` as TraceId;

export class InMemoryTracer implements Tracer {
  private readonly spans = new Map<SpanId, Span>();

  public startSpan(name: string, attrs: Record<string, unknown> = {}, parentSpanId?: SpanId): Span {
    const parent = parentSpanId === undefined ? undefined : this.spans.get(parentSpanId);
    const span: Span = {
      id: createSpanId(),
      traceId: parent?.traceId ?? createTraceId(),
      parentSpanId,
      name,
      startedAt: new Date(),
      status: "ok",
      attributes: attrs
    };

    this.spans.set(span.id, span);
    return span;
  }

  public endSpan(spanId: SpanId, status: SpanStatus, error?: string): void {
    const existing = this.spans.get(spanId);
    if (existing === undefined) {
      return;
    }

    const updated: Span = {
      ...existing,
      endedAt: new Date(),
      status,
      error
    };
    this.spans.set(spanId, updated);
  }

  public getSpan(spanId: SpanId): Span | undefined {
    return this.spans.get(spanId);
  }

  public getTrace(traceId: TraceId): Span[] {
    return [...this.spans.values()].filter((span) => span.traceId === traceId);
  }
}
