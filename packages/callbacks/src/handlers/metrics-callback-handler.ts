import type { CallbackHandler } from "../interfaces.js";
import type { CallbackEvent } from "../types.js";

export class MetricsCallbackHandler implements CallbackHandler {
  public readonly counts = new Map<string, number>();
  public readonly durationsMs = new Map<string, number[]>();
  private readonly starts = new Map<string, number>();

  public async onChainStart(event: Extract<CallbackEvent, { type: "onChainStart" }>): Promise<void> {
    this.starts.set(event.runId, Date.now());
    this.bump("onChainStart");
  }

  public async onChainEnd(event: Extract<CallbackEvent, { type: "onChainEnd" }>): Promise<void> {
    this.bump("onChainEnd");
    const start = this.starts.get(event.runId);
    if (start !== undefined) {
      const list = this.durationsMs.get(event.runId) ?? [];
      this.durationsMs.set(event.runId, [...list, Date.now() - start]);
    }
  }

  public async onNodeStart(): Promise<void> {
    this.bump("onNodeStart");
  }

  public async onNodeEnd(): Promise<void> {
    this.bump("onNodeEnd");
  }

  public async onNodeError(): Promise<void> {
    this.bump("onNodeError");
  }

  private bump(key: string): void {
    this.counts.set(key, (this.counts.get(key) ?? 0) + 1);
  }
}
