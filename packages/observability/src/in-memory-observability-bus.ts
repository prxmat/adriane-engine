import type { ObservabilityBus } from "./interfaces.js";
import type { ObservabilityEvent } from "./types.js";

export class InMemoryObservabilityBus implements ObservabilityBus {
  private readonly handlers = new Set<(event: ObservabilityEvent) => void>();

  public emit(event: ObservabilityEvent): void {
    for (const handler of this.handlers) {
      handler(event);
    }
  }

  public subscribe(handler: (event: ObservabilityEvent) => void): () => void {
    this.handlers.add(handler);
    return () => {
      this.handlers.delete(handler);
    };
  }
}
