---
sidebar_position: 1
title: One engine, many languages
description: The Rust engine parity contract for TypeScript, Python, and the C-ABI SDKs.
---

# One engine, many languages

Adriane is **one Rust engine** with multiple SDK surfaces. This page is the parity contract:
what is guaranteed identical, what is already exposed through the shared C ABI, and which
TypeScript features still require the next callback-runtime bridge before every SDK can expose
them faithfully.

## Shared engine contract

The graph model, the validator, the DSL compiler, the model policy, and the component and
prebuilt catalogs live once, in Rust. SDKs call into that same code:

- A graph that **validates** in one SDK validates identically in the others.
- A DSL document that **compiles** in one SDK compiles the same way in the others.
- `resolve_model` / model-policy decisions are the same given the same environment.
- The component and prebuilt catalogs are the same lists.
- Native component and prebuilt-agent runs execute in Rust, not as per-language rewrites.

There is no second implementation to drift. This is the whole point of the design.

## SDK families

| Family | Languages | Binding |
| --- | --- | --- |
| TypeScript | TypeScript / JavaScript | N-API package with callback-heavy graph execution |
| Python | Python | PyO3 wheel with JSON-in / JSON-out helpers |
| C ABI SDKs | Ruby, PHP, Lua, PowerShell, Go, C, C++, Zig, Swift, Objective-C, Java, Kotlin, Scala, C#, Elixir | `crates/c-api` shared dynamic library |

The C ABI SDKs intentionally start from the same callback-neutral surface that Python exposes.
That gives every language the real Rust engine without inventing a runtime bridge thirteen
times.

## Feature parity matrix

| Capability | TypeScript | Python | C ABI SDKs |
| --- | --- | --- | --- |
| Engine version | Yes | Yes | Yes |
| Validate `GraphDefinition` JSON | Yes | Yes | Yes |
| Compile graph YAML DSL | Yes | Yes | Yes |
| Resolve model tier / provider override | Yes | Yes | Yes |
| List component catalog | Yes | Yes | Yes |
| List prebuilt agents | Yes | Yes | Yes |
| Run native component | Yes | Yes | Yes |
| Run prebuilt micro-agent | Yes | Yes | Yes |
| Fluent graph builder | Yes | Planned | Planned |
| Host-language custom node handlers | Yes | Needs callback bridge | Needs callback bridge |
| Host-language conditional predicates | Yes | Needs callback bridge | Needs callback bridge |
| Streaming graph events | Yes | Needs callback bridge | Needs callback bridge |
| Checkpointer interface / resume / approval helpers | Yes | Needs callback bridge | Needs callback bridge |

The parity target is not optional: every TypeScript capability should either exist in each SDK
or be tracked as a bridge gap. The current C ABI covers the stable, callback-neutral part of the
engine first. The remaining TypeScript-only features all share the same root requirement: Rust
must be able to call safely back into the host language for handlers, predicates, streaming
events, and checkpoint lifecycle hooks.

## Callback boundary

Custom node handlers, conditional predicates, tool approval flows, and streaming are not simple
JSON functions. They require long-lived host callbacks, runtime ownership rules, threading
rules, error propagation, and allocator boundaries. TypeScript already has that through N-API.
The polyglot SDKs need a C callback runtime contract before those features can be exposed
without pretending that each language is identical.

Until that callback bridge exists, use:

- **TypeScript** for full custom graph execution, streaming, human gates, and checkpointers.
- **Python or C ABI SDKs** for validation, DSL compilation, model policy, catalogs, native
  components, and prebuilt micro-agent runs.

## Next

- [TypeScript SDK](./typescript-sdk)
- [Python SDK](./python-sdk)
- [Polyglot C ABI](./polyglot-c-abi)
