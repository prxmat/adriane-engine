/**
 * Demonstrates the first real migration flip (ADR 0002): when the `@adriane/napi`
 * native addon is present, the SDK validates graphs in Rust. Run after building it:
 *   cd crates && cargo build -p adriane-napi && cp target/debug/libadriane_napi.dylib bindings/adriane_napi.node
 *   pnpm --filter @adriane/graph-sdk exec node --import tsx examples/rust-validation.ts
 */
import { createGraph, rustValidatorActive } from "@adriane/graph-sdk";

console.log("Rust validator active:", rustValidatorActive());

const result = createGraph({ name: "broken" })
  .node("a", async () => ({}))
  .edge("a", "ghost") // dangling edge — caught by whichever validator is active
  .safeCompile();

console.log("compile success:", result.success);
if (!result.success) {
  console.log(
    "errors:",
    result.error.errors.map((error) => error.code)
  );
}
