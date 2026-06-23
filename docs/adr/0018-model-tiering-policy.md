# ADR 0018 — Capability tiers resolved by a ModelPolicy

- Status: Accepted (implemented)
- Date: 2026-06-22
- Deciders: Mathieu (owner)
- Builds on: [ADR 0005](0005-multi-provider-llm-gateway.md) (multi-provider gateway)

## Context

Agents should not hardcode concrete model ids (they rot, and they tie a graph to one
provider). Cost control (a recurring buyer ask) means routing cheap stages to small models
and reserving a frontier model for the few high-stakes steps — but that decision should be
declarative, resolved against whatever providers a deployment actually has.

## Decision

An `agentNode`/`ReActAgent` declares an abstract **capability tier** —
`"frontier" | "balanced" | "fast" | "creative"` — instead of (or alongside) a concrete model.
A **ModelPolicy** (`crates/llm-gateway/model_policy.rs`, mirrored TS-side) resolves the tier to
a concrete `{ provider, model }` against the providers available in the process env
(`available_from_env`): e.g. a Mistral-only environment maps every tier to the Mistral column.
An explicit `model` always wins over the tier. Resolution happens in the Rust bridge on the
native path (and TS-side on the fallback, pre-ADR-0016).

## Consequences

- A graph is portable across providers: the same tiered graph runs on whatever the deployment
  has, no per-environment edits.
- Cost control becomes declarative + per-stage (frontier only where it matters) — the lever
  behind a large share of the token/cost story (see ADR 0014).
- The policy is the single resolution point, so "balanced → which model?" is inspectable and
  consistent across the Rust engine and the SDK.

## Reserves

The tier→model mapping is a curated table that must track provider model churn (deprecations,
new tiers). Underused in practice today — worth surfacing in the SDK ergonomics so tiering is
the default reach rather than concrete model ids.
