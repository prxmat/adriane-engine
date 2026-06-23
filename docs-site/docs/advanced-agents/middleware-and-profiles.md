---
sidebar_position: 1
title: Middleware & profiles
description: Compose an agent's behaviour with profiles and efficiency middleware — governance stays built in.
---

# Middleware & profiles

Every agent node runs its ReAct loop through one ordered **middleware stack**. The stack has two
layers:

- a **governed** layer — PII redaction, the human-approval gate, the filesystem policy. It is
  **engine-injected and sealed**: you cannot add to it, remove from it, or turn it off.
- an **efficiency** layer — compression, terse output, context-budget trimming, reflection. This
  is the **user-tunable** layer you compose with `profile` and `middleware`.

That split is the *governed-by-construction* guarantee: an ungoverned agent is unrepresentable.
A user can only ever add efficiency middleware — the governance layer is always present.

```ts
import { createGraph, DefaultLLMGateway } from "@adriane-ai/graph-sdk";

createGraph({ name: "deep" })
  .agentNode("worker", {
    llm: new DefaultLLMGateway(),
    prompt: { system: "Investigate and report." },
    profile: "governed-deep"   // tier + efficiency middleware + suspend + fs, in one word
  })
  .compile();
```

## Profiles

A **profile** is a named bundle: a model tier, a set of efficiency middleware, and
suspend / filesystem defaults. It is the one-word way to dial an agent's posture. The governed
layer is identical across all profiles — you cannot buy out of governance.

| Profile | Tier | Efficiency middleware | Suspends for approval | Filesystem |
| --- | --- | --- | --- | --- |
| `fast` | `fast` | compress + terse + context-budget (4k) | no | off |
| `frontier-careful` | `frontier` | context-budget (16k) + reflection, **no compression** | yes | off |
| `governed-deep` | `balanced` | compress + terse + context-budget (12k) + reflection | yes | **on** |

- `fast` — high-throughput, low-stakes prose. Compresses and trims aggressively.
- `frontier-careful` — high-stakes reasoning where lossy compression is unsafe. Keeps the full
  text, reflects on its own answer, and suspends for human approval.
- `governed-deep` — the deep-agent one-liner: a governed agent with the virtual filesystem, the
  full efficiency stack, reflection, and suspend-on-approval.

An **explicit field always wins over the profile**. Set `tier`, `suspendForApproval`, `enableFs`,
`outputStyle`, `contextBudget`, or `middleware` directly and your value overrides the profile's
default:

```ts
.agentNode("worker", {
  llm,
  prompt: { system: "…" },
  profile: "governed-deep",
  tier: "frontier",          // override balanced → frontier
  suspendForApproval: false  // override the profile's suspend default
})
```

## Efficiency middleware

`middleware` appends efficiency middleware to whatever the profile (and the flat
`outputStyle` / `contextBudget` knobs) already contribute. The kinds:

| Kind | Effect |
| --- | --- |
| `{ kind: "compress" }` | Route messages through the prompt-compression service (a no-op when it is not configured — see [token optimization](#token-optimization-knobs)). |
| `{ kind: "terse" }` | Append a compact-output directive to the system prompt. Lossy — prose only, not code. |
| `{ kind: "contextBudget", params: { chars } }` | Cap the agent's seed message (the injected `Input` / `State` dump) to `chars` characters. |
| `{ kind: "reflection", params?: { threshold? } }` | One self-critique after the run — see [reflection](#reflection). |

```ts
.agentNode("writer", {
  llm,
  prompt: { system: "Draft a release note." },
  middleware: [{ kind: "terse" }, { kind: "contextBudget", params: { chars: 8000 } }]
})
```

The SDK desugars `profile` + the flat `outputStyle` / `contextBudget` knobs + your explicit
`middleware` into one ordered, **deduplicated** list (most specific wins: explicit `middleware`
overrides the flat knobs, which override the profile). The order within the efficiency layer does
not change behaviour — the hooks act on disjoint parts of the request.

### Governance kinds are rejected

The governance middleware — redaction, the approval gate, the filesystem policy — are *not* part
of the `middleware` type. They are engine-injected. Passing one (e.g. from untyped JavaScript)
throws:

```ts
import { GovernanceMiddlewareRejectedError } from "@adriane-ai/graph-sdk";

// { kind: "redact" } is not an EfficiencyMiddlewareSpec — the SDK rejects it at build time.
```

This is what makes an ungoverned stack unrepresentable: you cannot express "an agent without the
approval gate", and you cannot smuggle a governance middleware into the user layer.

## The lifecycle hooks

Under the hood the stack drives seven pass-through hooks around the ReAct loop. You rarely touch
them directly from the SDK (you compose with profiles and `middleware`), but they explain *where*
each behaviour runs:

| Hook | When | Used by |
| --- | --- | --- |
| `before_run` | once, after state injection, before the loop | context-budget trim |
| `before_model` | before each `gateway.complete()` | redaction (governed), compression, terse |
| `after_model` | after each completion | redaction hydrate (governed) |
| `before_tool` | before a tool executes | the approval gate (governed), filesystem policy (governed) |
| `after_tool` | after a tool's handler returns | observation transforms |
| `on_iteration` | at each loop-turn end | loop-detection / budget |
| `after_run` | after the loop, before the result | reflection |

Request-path hooks run governed → efficiency (governance outermost); response-path hooks run in
reverse (onion semantics). The approval gate is **intrinsic** to `before_tool` — it applies even
when no middleware is installed, so a bare agent still gates its sensitive tools.

## Token optimization knobs

Compression and context-budget are the two token levers, both surfaced as efficiency middleware
(and as the flat `outputStyle: "terse"` / `contextBudget` knobs, which desugar to the same list):

- **`terse`** appends a compact-output directive to the system prompt. Lossy, so the profiles only
  use it where prose quality is not at stake (`fast`, `governed-deep`), never on `frontier-careful`.
- **`contextBudget`** caps the injected seed message so an unbounded channel map is not re-fed to
  the model on every turn.
- **`compress`** routes message content through an external prompt-compression service
  (LLMLingua-style), configured by the `ADRIANE_LLMLINGUA_URL` env var. It **fails open** — when
  the service is not configured the request passes through unchanged, so a `compress` entry is
  simply a no-op.

## Reflection

`{ kind: "reflection" }` runs **one self-critique** after the agent finishes: it asks the model to
score its own output, and when the critique rejects it, annotates the reasoning with a
`reflection:needs_review:<issues>` marker. It does **not** force a suspension — route on the marker
with a conditional edge if you want a weak answer to reach a human gate:

```ts
.agentNode("answer", { llm, prompt: { system: "Answer." }, middleware: [{ kind: "reflection" }] })
.humanGate("review")
.conditionalEdge(
  "answer", "review", "weak",
  (s) => s.channels.agentResult.reasoning.includes("reflection:needs_review")
)
```

Reflection is **additive** — it does not replace the standalone reflection node (the full
critique → revise loop). It is best-effort and **fails open**: a critique-call error never fails
the run.

## Next

- [The governed virtual filesystem →](./governed-filesystem)
- [Deep agents: todos & tasks →](./deep-agents)
- [Builder API reference](/docs/reference/builder-api)
