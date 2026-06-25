import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  docQaReferenceDefinition,
  replayCatalogGraph,
  runCatalogGraph,
  rustEngineAvailable,
  verifyReplayDecisions,
  type CatalogRunOutcome,
  type RunId
} from "./index.js";

/**
 * Replay-as-evidence (ADR 0038) — the napi PLUMBING proven end-to-end on the Rust engine.
 *
 * Records a governed catalog run (`ADRIANE_LLM_RECORD`) so the engine journals its LLM outputs +
 * timestamp sequence, then drives {@link replayCatalogGraph} through `engineReplay` and asserts the
 * deterministic fork comes back without re-sampling. This guards the feature-detected replay path
 * from silently rotting; it does NOT assert full faithfulness from the run's *initial* checkpoint —
 * that needs the control plane's persisted checkpoint history + attestation chain and is L5's e2e.
 *
 * Skipped when the native addon is absent. Tolerant of an addon predating replay support.
 */

const QUESTION = "How does Adriane resume a run after a crash or an approval?";
const DOCUMENTS = "Adriane is a resumable agent graph runtime. It checkpoints after every node.";
const PROVIDER_KEYS = ["MISTRAL_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY", "ADRIANE_USE_OLLAMA"] as const;

const lacksReplaySupport = (error: unknown): boolean =>
  error instanceof Error && /no replay support|engineReplay/i.test(error.message);

describe("@adriane-ai/graph-sdk — replay-as-evidence napi round-trip (ADR 0038)", () => {
  const saved: Record<string, string | undefined> = {};

  beforeEach(() => {
    for (const key of [...PROVIDER_KEYS, "ADRIANE_LLM_RECORD"]) {
      saved[key] = process.env[key];
      delete process.env[key];
    }
  });

  afterEach(() => {
    for (const key of [...PROVIDER_KEYS, "ADRIANE_LLM_RECORD"]) {
      if (saved[key] === undefined) delete process.env[key];
      else process.env[key] = saved[key];
    }
  });

  (rustEngineAvailable() ? it : it.skip)(
    "records a run's journal, then replays it deterministically through engineReplay",
    async () => {
      const definition = docQaReferenceDefinition();

      // 1. RECORD: the engine journals each agent's LLM I/O + the run's timestamp sequence.
      process.env.ADRIANE_LLM_RECORD = "1";
      const recorded = await runCatalogGraph(definition, {
        runId: "run_replay_evidence" as RunId,
        initialData: { question: QUESTION, documents: DOCUMENTS }
      });
      delete process.env.ADRIANE_LLM_RECORD;

      expect(recorded.status).toBe("completed");
      expect(recorded.usedRustEngine).toBe(true);
      // Record mode surfaced a non-trivial journal (`{ decisions, clock }`) for the control plane.
      expect(typeof recorded.replayJournal).toBe("string");
      expect((recorded.replayJournal ?? "").length).toBeGreaterThan(0);

      // 2. REPLAY: re-feed the journal through the napi `engineReplay` bridge.
      let replayed: CatalogRunOutcome;
      try {
        replayed = await replayCatalogGraph(
          definition,
          recorded.state,
          "cp_replay_evidence_seed",
          recorded.replayJournal ?? "{}"
        );
      } catch (error) {
        if (lacksReplaySupport(error)) return; // addon predates replay — plumbing not present, skip.
        throw error;
      }

      expect(replayed.usedRustEngine).toBe(true);
      expect(replayed.status).toBe("completed");
      // `replay_from` forks a NEW deterministic run id (`<run>:fork:<seq>`) — the original is untouched.
      expect(String(replayed.state.runId).startsWith("run_replay_evidence:fork:")).toBe(true);
    }
  );

  it("verifyReplayDecisions confirms an attested chain reproduced by a replay", () => {
    // The faithfulness comparator over the two decision sets the control plane will derive (L5).
    const attested = [{ status: "approved", subject: "ship the release notes" }];
    const replayed = [{ status: "approved", subject: "ship the release notes" }];
    expect(verifyReplayDecisions(attested, replayed).ok).toBe(true);
    expect(verifyReplayDecisions(attested, [{ status: "rejected", subject: "ship the release notes" }]).ok).toBe(
      false
    );
  });
});
