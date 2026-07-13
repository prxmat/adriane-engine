// LLM Council — governed deliberation (ADR 0013 / ADR 0061). A council dispatches a query to N member
// agents (fan-out), has reviewers rank the ANONYMIZED field, aggregates the ranks, and a chair
// synthesizes the final answer. This module holds the two DETERMINISTIC pure steps — anonymize+shuffle
// and aggregate-ranks — no model, no I/O, replay-faithful. They ARE the "not an ensemble trick" core
// of ADR 0013 (an anonymized peer-review + a consensus aggregate). The graph builder that wires them
// between the agent fan-outs is E2b (it needs the dynamic-N builder-typing resolved cleanly).

/** One member's answer to the query. */
export type MemberAnswer = { memberId: string; content: string };

/** An answer stripped of its author, relabeled + shuffled so a reviewer can't favour its own. */
export type AnonymizedAnswer = { label: string; content: string; memberId: string };

/** Deterministic 32-bit hash (FNV-1a) — for a replay-faithful shuffle. */
const hash = (text: string): number => {
  let h = 2166136261;
  for (let i = 0; i < text.length; i += 1) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
};

const LABELS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/**
 * Strip authorship, relabel `A,B,C,…`, and shuffle deterministically by `seed` (so the same run
 * replays identically). Reviewers see only `{ label, content }`; the `memberId` is retained so the
 * control plane can de-anonymize the audit trail after ranking — never shown to a reviewer.
 */
export const anonymizeAndShuffle = (answers: MemberAnswer[], seed: string): AnonymizedAnswer[] => {
  const ordered = [...answers].sort((a, b) => {
    const ha = hash(`${seed}:${a.memberId}`);
    const hb = hash(`${seed}:${b.memberId}`);
    return ha === hb ? a.memberId.localeCompare(b.memberId) : ha - hb;
  });
  return ordered.map((answer, index) => ({
    label: LABELS[index] ?? `M${index}`,
    content: answer.content,
    memberId: answer.memberId
  }));
};

/**
 * Aggregate reviewer rankings into a consensus order (Borda count). Each ranking is an ordered list
 * of labels, best-first; a label at position `p` of `n` scores `n - p`. Unranked labels score 0.
 * Returns the labels best-first; ties break by label asc (deterministic). A ranking's duplicate or
 * unknown labels are ignored.
 */
export const aggregateRanks = (rankings: string[][], labels: string[]): string[] => {
  const valid = new Set(labels);
  const scores = new Map<string, number>(labels.map((l) => [l, 0]));
  for (const ranking of rankings) {
    const seen = new Set<string>();
    const clean = ranking.filter((l) => valid.has(l) && !seen.has(l));
    for (const l of clean) seen.add(l);
    const n = clean.length;
    clean.forEach((label, position) => {
      scores.set(label, (scores.get(label) ?? 0) + (n - position));
    });
  }
  return [...labels].sort((a, b) => {
    const sa = scores.get(a) ?? 0;
    const sb = scores.get(b) ?? 0;
    return sb === sa ? a.localeCompare(b) : sb - sa;
  });
};

/**
 * Parse a reviewer's free-text reply into an ordered list of labels (the labels it names, in order,
 * deduped) — tolerant of prose like "I rank B first, then A, then C". Unknown labels are dropped.
 */
export const parseRanking = (text: string, labels: string[]): string[] => {
  const valid = new Set(labels);
  const seen = new Set<string>();
  const out: string[] = [];
  for (const token of text.toUpperCase().split(/[^A-Z]+/)) {
    const head = token[0];
    if (head !== undefined && valid.has(head) && !seen.has(head)) {
      seen.add(head);
      out.push(head);
    }
  }
  return out;
};
