// LLM Council — governed deliberation (ADR 0013 / ADR 0061). A council dispatches a query to N member
// agents (fan-out), has reviewers rank the ANONYMIZED field, aggregates the ranks, and a chair
// synthesizes the final answer. This module holds the two DETERMINISTIC pure steps — anonymize+shuffle
// and aggregate-ranks — no model, no I/O, replay-faithful (the "not an ensemble trick" core of ADR
// 0013) — plus the `council(...)` graph builder that wires them between the agent fan-outs.

import { createGraph } from "./builder.js";
import type { CompiledGraph } from "./compiled-graph.js";
import type { AgentNodeConfig } from "./agent-node.js";

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

/** A council seat (member / reviewer / chair) — an agent-node config minus its output channel. */
export type CouncilSeat = Omit<AgentNodeConfig, "outputChannel">;

export type CouncilOptions = {
  /** Channel holding the query text. Default `"query"`. */
  queryChannel?: string;
  /** The member agents (fixed N). Each answers the query independently, in parallel. */
  members: CouncilSeat[];
  /** Reviewers that rank the anonymized field. Default: one reviewer per member. */
  reviewers?: CouncilSeat[];
  /** The chair that synthesizes the final answer from the aggregated field (writes `answer`). */
  chair: CouncilSeat;
  /** Suspend on a human gate before the chair synthesizes (high-stakes). Default false. */
  humanGate?: boolean;
  /** Seed for the deterministic anonymize-shuffle (replay). Default `"council"`. */
  seed?: string;
};

const memberChannel = (i: number): string => `member_${i}`;
const reviewChannel = (i: number): string => `review_${i}`;

/** Read an agent-result channel's answer text, tolerating shapes. */
const readContent = (value: unknown): string => {
  if (typeof value === "string") return value;
  if (value !== null && typeof value === "object") {
    const record = value as Record<string, unknown>;
    if (typeof record.content === "string") return record.content;
    if (typeof record.output === "string") return record.output;
  }
  return "";
};

/**
 * Build a governed LLM Council (ADR 0013 / ADR 0061): dispatch → members (fan-out) → anonymize+shuffle
 * → reviewers (fan-out, rank the field) → aggregate → [optional human gate] → chair synthesis.
 * Members/reviewers/chair are native agent nodes (governed: a member never reviews its own answer,
 * every seat audited); anonymize/aggregate are the deterministic pure helpers above. Fixed N (the
 * member list length) via the runtime fan-out. Built by mutation so a per-seat loop stays type-clean.
 */
export const council = (options: CouncilOptions): CompiledGraph => {
  const query = options.queryChannel ?? "query";
  const reviewers = options.reviewers ?? options.members;
  const seed = options.seed ?? "council";
  const builder = createGraph({ name: "council" });

  builder.channel(query, { type: "string", default: "" });
  builder.channel("field", { type: "json", default: [] });
  builder.channel("aggregate", { type: "json", default: [] });

  // dispatch: a single-headed entry the members fan out from.
  builder.node("dispatch", async () => ({}));
  options.members.forEach((seat, i) => {
    builder.agentNode(memberChannel(i), { ...seat, outputChannel: memberChannel(i) });
  });

  builder.node("anonymize", async (_input, state) => {
    const channels = state.channels as Record<string, unknown>;
    const answers = options.members.map((_, i) => ({
      memberId: memberChannel(i),
      content: readContent(channels[memberChannel(i)])
    }));
    return { field: anonymizeAndShuffle(answers, seed) };
  });

  reviewers.forEach((seat, i) => {
    builder.agentNode(reviewChannel(i), { ...seat, outputChannel: reviewChannel(i) });
  });

  builder.node("aggregate", async (_input, state) => {
    const channels = state.channels as Record<string, unknown>;
    const field = (channels.field as AnonymizedAnswer[] | undefined) ?? [];
    const labels = field.map((f) => f.label);
    const rankings = reviewers.map((_, i) =>
      parseRanking(readContent(channels[reviewChannel(i)]), labels)
    );
    return { aggregate: aggregateRanks(rankings, labels) };
  });

  if (options.humanGate === true) {
    builder.humanGate("gate");
  }
  builder.agentNode("chair", { ...options.chair, outputChannel: "answer" });

  // Wire the two fan-outs + the tail.
  builder.edge("dispatch", memberChannel(0));
  builder.fanOut(
    "dispatch",
    options.members.map((_, i) => memberChannel(i)),
    "anonymize"
  );
  builder.edge("anonymize", reviewChannel(0));
  builder.fanOut(
    "anonymize",
    reviewers.map((_, i) => reviewChannel(i)),
    "aggregate"
  );
  if (options.humanGate === true) {
    builder.edge("aggregate", "gate");
    builder.edge("gate", "chair");
  } else {
    builder.edge("aggregate", "chair");
  }

  builder.entry("dispatch");
  return builder.compile();
};
