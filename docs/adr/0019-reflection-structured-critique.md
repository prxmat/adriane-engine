# ADR 0019 — Reflection / self-correction with a structured critique

- Status: Accepted (implemented)
- Date: 2026-06-22
- Deciders: Mathieu (owner)

## Context

`agents-core` ships reflection (`createReflectionNode` / `ReflectionAgent`) and
self-correction — a generate → critique → revise loop. The original acceptance test was a
**substring heuristic** on the critique text (`"problem"` / `"retry"`): brittle (wording-
dependent) and not tunable.

## Decision

The critique is a **structured verdict** `{ ok, score, issues }`. The reviser accepts when
`ok || score >= scoreThreshold` (default 0.8) and re-injects `issues` into the next revise
turn. A **substring fallback** preserves backward compatibility when the model returns
non-JSON. Implemented with parity across both runtimes: TS `reflection-node.ts`
(`scoreThreshold`, `parseReflectionCritique`, `critiqueRequestsRevision`, `__reflectionIssues`)
and Rust `reflection.rs` (`Critique`, `parse_critique`, `with_score_threshold`, tolerant JSON
extraction from prose/markdown).

## Consequences

- A configurable bar (`scoreThreshold`) replaces a magic substring — "make it stricter" is now
  a number, not a prompt hack.
- The loop is more robust: structured `issues` give the reviser concrete targets; the fallback
  keeps weak/non-JSON models working.
- Parity TS↔Rust keeps the SDKs behaviour-identical (the one-engine invariant).

## Reserves

The critique relies on the judging model emitting usable JSON; the fallback degrades to the
old heuristic for models that won't. Threshold tuning is per-use-case, not universal.
