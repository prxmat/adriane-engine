# ADR 0017 — `.fanOut()` SDK builder for native parallel branches

- Status: Accepted (implemented)
- Date: 2026-06-22
- Deciders: Mathieu (owner)
- Builds on: [ADR 0015](0015-rust-runtime-parity-fanout-subgraphs-streaming.md) (the runtime already executes static fan-out concurrently)

## Context

The Rust runtime has executed static fan-out since the parity work (ADR 0015): a node with
`fanOut = { parallelTo, joinAt }` runs its branches concurrently (`join_all`), merges updates
in declared order (deterministic — ADR 0008 invariant), then jumps to the join node. But the
graph-sdk builder did not expose it, so an SDK user's only way to run N parallel LLM calls
was an internal `Promise.all` inside a single node — which the benchmarks/council had to do as
a workaround. Fan-out was a runtime capability with no public surface.

## Decision

Add `.fanOut(from, parallelTo[], joinAt)` to the builder. It sets the `fanOut` field on an
existing `from` node; `parallelTo`/`joinAt` are validated at compile (graph-core validator).
The Rust engine executes it verbatim — **no engine change**, pure builder sugar. It is the
supported way to run N parallel branches (each an `agentNode`) on the public SDK with no raw
provider `fetch`. A new `UnknownNodeError` is thrown when `from` was never added.

## Consequences

- Fan-out is **static** (a fixed build-time set of branches). It fits fixed-parallel stages
  (e.g. classify category/priority/sentiment, or a council with N fixed members). Dynamic
  per-item map over a runtime-sized list remains a separate primitive (`send`/inbox; a true
  dynamic parallel map is future runtime work).
- Determinism preserved: branches see the same pre-fan-out snapshot; updates fold in declared
  order regardless of which finishes first.

## Reserves

Calling it "fan-out" while it is static can mislead users expecting dynamic map. Documented in
the method's doc comment + the benchmarks README fan-out note.
