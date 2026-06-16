/**
 * @deprecated The TypeScript graph-adriane DSL compiler is deprecated as part of the
 * execution engine. Graph YAML compilation has moved to the Rust `crates/graph-adriane`
 * crate, used by `@adriane/graph-sdk` through the `@adriane/napi` native addon; this
 * package remains only as a fallback when that native addon is absent. New code should
 * compile graph YAML via `@adriane/graph-sdk`, not by importing this engine directly.
 * See `docs/adr/0003-ts-engine-deprecated-sdk-on-rust.md`.
 */
export * from "./ast/types";
export * from "./parser/ref";
export * from "./parser/build-graph-ast";
export * from "./validator/types";
export * from "./validator/validate-graph-ast";
export * from "./transformer/transform-graph";
export * from "./compiler/compile-graph-file";
