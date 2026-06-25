# ADR 0039 — mthds as an LLM-friendly authoring frontend that compiles to GraphDefinition

- **Status:** Proposed (future phase — not built now; captured per owner intent "un jour, générer des graphes par LLM via un standard")
- **Date:** 2026-06-25
- **Relates to:** `graph-adriane`/`lang-adriane` (the existing DSL compilers this sits beside), [0029 governed structured output](./0029-governed-structured-output.md) (the typed-output seam mthds Concepts align with), [0037 product-engine-consumption](./0037-product-engine-consumption.md)

## Context

[mthds](https://mthds.ai/latest/) (the open spec behind Pipelex, MIT) is a declarative TOML language for AI methods built on two primitives:

- **Concepts** — semantically-typed data "named after real domain things" (e.g. `ContractClause`), with **refinement** typing (`NonCompeteClause` refines `ContractClause`) so pipes compose wherever types are compatible.
- **Pipes** — deterministic orchestration steps with explicit typed inputs/outputs: `PipeLLM`, `PipeExtract`, `PipeSequence`, `PipeCondition`, with `batch_over`/`batch_as` for parallel-over-collection, and built-in **structured generation**. Prompts reference typed inputs via `@` notation. Grouped under **Domains** (namespaces); packaged via `METHODS.toml` + `methods.lock`.

The owner finds the mthds authoring model **clearer than Adriane's current YAML DSL** — particularly the first-class, named, refinement-typed Concepts and the `@`-typed prompt inputs.

**This ADR is explicitly NOT about standardising Adriane's own DSL** (that would commoditise the language; Adriane's moat is the **governed, resumable runtime**, not the authoring format). It is about **consuming mthds as an input frontend**: a `mthds` TOML method compiles into a governed Adriane `GraphDefinition`, so the runtime's gates / attestation / replay-as-evidence apply to it for free.

**Primary driver:** make **LLM-generated graphs** practical. Asking an LLM to emit a clear, typed, public standard (mthds) and compiling that into the governed graph is far more robust than asking it to emit Adriane's bespoke YAML — the type system catches malformed graphs at compile time, and the standard is documented outside our repo (better in-context grounding for the model).

## Decision (direction, for a future phase)

Add a **mthds frontend** as a new DSL compiler beside `graph-adriane`, following the same `parser → ast → validator → transformer → compiler` pipeline, emitting a validated `GraphDefinition`. Proposed mapping (to be finalised against `spec/mthds-format` at build time):

| mthds | Adriane |
| --- | --- |
| **Concept** (typed data) | a **channel** + its Zod schema (typed state); refinement → schema extension / compatibility check |
| **PipeLLM** | an agent/tool node routed through `llm-gateway`, with the Concept as its structured-output schema (ties into [ADR 0029](./0029-governed-structured-output.md)) |
| **PipeExtract** | a tool node (OCR/parse) |
| **PipeSequence** (`steps = [{pipe, result}]`) | nodes + sequential edges; `result` names the output channel |
| **PipeCondition** | a named `ConditionRegistry` predicate + conditional edges (never eval'd code) |
| `batch_over` / `batch_as` | fan-out / `send` over a collection channel |
| **Domain** | graph/namespace id |
| `@typed_input` in prompts | a channel reference resolved at compile time |

Governance is additive and orthogonal: a compiled mthds graph runs on the same engine, so human-gate nodes, the Ed25519 attestation chain, and (post-[0038](./0038-replay-as-evidence.md)) replay-as-evidence all apply. A future extension can let mthds annotate a pipe as **gated** (→ an approval node), giving "typed authoring + governance" in one document — a combination neither mthds-alone nor a bare governed runtime offers.

## Why this is upside without ceding the moat

- Adriane's differentiator is the **governed runtime**, not the DSL → adopting a clearer public authoring language on top is pure gain (better human authoring, robust LLM generation, ecosystem familiarity).
- mthds is MIT / open-spec → no lock-in; Adriane consumes it, does not depend on Pipelex's runtime.
- Adriane keeps `graph-adriane`/`lang-adriane` (no breakage); mthds is an **additional** frontend.

## Impact

- **Additive**: a new engine compiler package (e.g. `mthds-adriane`), mirroring `graph-adriane`. No runtime invariant change; no change to existing DSLs. New public API surface (the compiler) → mandatory human review at build time.
- **Effort**: a real compiler (TOML parse + Concept/refinement type system → Zod + the pipe→node mapping). Sized as its own phase, not a tour-of-work increment.

## Open questions (resolve at build)

1. Full grammar mapping — fetch `spec/mthds-format` + `spec/manifest-format` and pin the exact pipe fields + refinement semantics.
2. Refinement typing → Zod: structural extension vs nominal compatibility check.
3. `METHODS.toml` packaging / `methods.lock` — support the package manifest, or just single-method compile, for v1?
4. Governance annotations: does mthds get an Adriane-specific `gated`/`approval` extension, or stay vanilla mthds (governance applied by graph wiring outside the method)?
5. Direction of travel: import-only (mthds → graph), or also export (graph → mthds) for round-trip / LLM-edit loops?

## Consequences

- A documented, typed, public authoring path that an LLM can target reliably, compiling into the governed runtime — the cleanest answer to "generate governed graphs by LLM."
- Deferred until after the in-flight MVP work (attestation hardening, replay-as-evidence, the engine repoint). Captured here so the design intent is not lost.
