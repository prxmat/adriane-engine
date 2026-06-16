import { defineConfig } from "tsup";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const src = (name: string, entry = "src/index.ts"): string => resolve(here, "..", name, entry);

// Inline every @adriane/* workspace package into a single self-contained artifact,
// resolving each to its TypeScript SOURCE (the packages' own dist/ is never relied
// upon, and the `workspace:*` specifiers dissolve away — nothing else must publish).
const workspaceAlias: Record<string, string> = {
  "@adriane/graph-core": src("graph-core"),
  "@adriane/graph-runtime": src("graph-runtime"),
  "@adriane/agents-core": src("agents-core"),
  "@adriane/llm-gateway": src("llm-gateway"),
  "@adriane/artifact-store": src("artifact-store"),
  "@adriane/approval-engine": src("approval-engine")
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
  // `@adriane/db` and `@adriane/config` are PRIVATE workspace packages and must never
  // be inlined into the public bundle — keep them external so the published SDK never
  // embeds the DB schema. (No public SDK source imports them anymore; this is a guard.)
  external: [
    "@adriane/napi",
    "@adriane/db",
    "@adriane/config",
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
