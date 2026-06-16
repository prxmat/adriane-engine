import type { CallbackHandler, CallbackManager } from "./interfaces.js";
import type { CallbackEvent } from "./types.js";

export class InMemoryCallbackManager implements CallbackManager {
  private readonly handlers = new Set<CallbackHandler>();

  public constructor(
    handlers: CallbackHandler[] = [],
    private readonly inheritedTags: string[] = [],
    private readonly inheritedMetadata: Record<string, unknown> = {}
  ) {
    for (const handler of handlers) {
      this.handlers.add(handler);
    }
  }

  public addHandler(handler: CallbackHandler): void {
    this.handlers.add(handler);
  }

  public removeHandler(handler: CallbackHandler): void {
    this.handlers.delete(handler);
  }

  public async emit(event: CallbackEvent): Promise<void> {
    const merged: CallbackEvent = {
      ...event,
      tags: [...this.inheritedTags, ...(event.tags ?? [])],
      metadata: {
        ...this.inheritedMetadata,
        ...(event.metadata ?? {})
      }
    };

    for (const handler of this.handlers) {
      try {
        const fn = handler[merged.type];
        if (typeof fn === "function") {
          await fn(merged as never);
        }
      } catch {
        // swallow callback errors by design
      }
    }
  }

  public createChild(tags: string[] = [], metadata: Record<string, unknown> = {}): CallbackManager {
    return new InMemoryCallbackManager(
      [...this.handlers],
      [...this.inheritedTags, ...tags],
      { ...this.inheritedMetadata, ...metadata }
    );
  }
}
