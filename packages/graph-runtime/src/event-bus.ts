import type { EventBus } from "./interfaces.js";
import type { RunEvent } from "./types.js";

export class InMemoryEventBus implements EventBus {
  private readonly subscribers = new Set<(event: RunEvent) => void>();

  public emit(event: RunEvent): void {
    for (const subscriber of this.subscribers) {
      subscriber(event);
    }
  }

  public subscribe(handler: (event: RunEvent) => void): () => void {
    this.subscribers.add(handler);
    return () => {
      this.subscribers.delete(handler);
    };
  }
}
