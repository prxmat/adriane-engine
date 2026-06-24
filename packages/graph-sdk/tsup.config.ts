import { defineConfig } from "tsup";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const src = (name: string, entry = "src/index.ts"): string => resolve(here, "..", name, entry);

// Inline every @adriane-ai/* workspace package into a single self-contained artifact,
// resolving each to its TypeScript SOURCE (the packages' own dist/ is never relied
// upon, and the `workspace:*` specifiers dissolve away — nothing else must publish).
const workspaceAlias: Record<string, string> = {
  "@adriane-ai/graph-core": src("graph-core"),
  "@adriane-ai/graph-runtime": src("graph-runtime"),
  "@adriane-ai/agents-core": src("agents-core"),
  "@adriane-ai/llm-gateway": src("llm-gateway"),
  "@adriane-ai/artifact-store": src("artifact-store"),
  "@adriane-ai/approval-engine": src("approval-engine"),
  // ADR 0037: inlined so the product reaches them through the graph-sdk door (zero @adriane-ai deps).
  "@adriane-ai/search": src("search"),
  "@adriane-ai/memory-store": src("memory-store")
};

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["esm"],
  target: "es2022",
  platform: "node",
  // dts resolves the inlined packages' types through tsconfig.base.json `paths`.
  tsconfig: "tsconfig.build.json",
  dts: true,
  sourcemap: true,
  clean: true,
  treeshake: true,
  splitting: false,
  // Real npm dependencies stay external (declared in package.json `dependencies`);
  // the native engine addon is loaded lazily via createRequire and must never be
  // bundled — its absence is a graceful fall back to the in-bundle TS engine.
  // `@adriane-ai/db` and `@adriane-ai/config` are PRIVATE workspace packages and must never
  // be inlined into the public bundle — keep them external so the published SDK never
  // embeds the DB schema. (No public SDK source imports them anymore; this is a guard.)
  external: [
    "@adriane-ai/napi",
    "@adriane-ai/db",
    "@adriane-ai/config",
    "@anthropic-ai/sdk",
    "zod",
    "pg",
    "pg-native",
    "drizzle-orm"
  ],
  esbuildOptions(options) {
    options.alias = workspaceAlias;
  }
});
