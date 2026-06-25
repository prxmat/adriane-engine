//! Replay-as-evidence (ADR 0038): the FAITHFULNESS check — does a deterministic replay reproduce
//! the SAME governed decisions the run was attested for?
//!
//! This is deliberately SEPARATE from `verifyChain` (the tamper-evidence check over the Ed25519
//! hash-chained attestation). Two independent guarantees:
//!   - `verifyChain`        — the attested records were not altered (tamper-evidence).
//!   - `verifyReplayDecisions` — the signed decisions are the ones the run reproduces (faithfulness).
//!
//! A decision is its `{ status, subject }`. The caller derives `subject` the SAME way the
//! attestation does (the description, else canonical JSON of the subject) for both sides, so this
//! helper is a pure ordered-equivalence over strings — it does NOT re-derive subjects or touch
//! crypto. `decidedAt` / `resolvedBy` / `approvalId` are intentionally NOT compared: they are
//! wall-clock / human / random facts a re-execution cannot (and should not) reproduce.

/** One governance decision, reduced to what a replay can faithfully reproduce. */
export type ReplayDecision = {
  /** `"approved" | "rejected"`. */
  status: string;
  /** The decision subject, derived identically to the attestation (`description || canonicalJson`). */
  subject: string;
};

/** The result of comparing the attested decisions to the replayed ones, in order. */
export type VerifyReplayResult = {
  /** True iff every attested decision is reproduced, in the same order, by the replay. */
  ok: boolean;
  attested: ReplayDecision[];
  replayed: ReplayDecision[];
  /** Per-position divergences (missing on either side, or a status/subject differs). */
  mismatches: { index: number; attested?: ReplayDecision; replayed?: ReplayDecision }[];
};

/**
 * Compare the ordered `{ status, subject }` decision sets of the attested chain and a replayed run.
 * Order matters (a dropped, reordered, or status-flipped decision is a mismatch). Pure + crypto-free.
 */
export const verifyReplayDecisions = (
  attested: ReplayDecision[],
  replayed: ReplayDecision[]
): VerifyReplayResult => {
  const mismatches: VerifyReplayResult["mismatches"] = [];
  const length = Math.max(attested.length, replayed.length);
  for (let index = 0; index < length; index += 1) {
    const a = attested[index];
    const r = replayed[index];
    if (a === undefined || r === undefined || a.status !== r.status || a.subject !== r.subject) {
      mismatches.push({ index, attested: a, replayed: r });
    }
  }
  return { ok: mismatches.length === 0, attested, replayed, mismatches };
};
