import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

const fromHere = (relativePath: string): string => fileURLToPath(new URL(relativePath, import.meta.url));

/**
 * Resolve workspace dependencies to their TypeScript source rather than the built
 * `dist/` (which can be stale). This mirrors the tsconfig path aliases the rest of
 * the repo relies on, so the SDK's tests always exercise current source.
 */
export default defineConfig({
  test: {
    environment: "node"
  },
  resolve: {
    alias: {
      // Resolve the SDK's own package name to source too, so example files (which
      // import the public `@adriane-ai/graph-sdk` entry) and their tests exercise current
      // source instead of a possibly-stale `dist/`.
      "@adriane-ai/graph-sdk": fromHere("./src/index.ts"),
      "@adriane-ai/graph-core": fromHere("../graph-core/src/index.ts"),
      "@adriane-ai/graph-runtime": fromHere("../graph-runtime/src/index.ts"),
      "@adriane-ai/agents-core": fromHere("../agents-core/src/index.ts"),
      "@adriane-ai/llm-gateway": fromHere("../llm-gateway/src/index.ts"),
      "@adriane-ai/model-core": fromHere("../model-core/src/index.ts"),
      // ADR 0037: now re-exported through the index, so tests resolving the index need them too.
      "@adriane-ai/approval-engine": fromHere("../approval-engine/src/index.ts"),
      "@adriane-ai/artifact-store": fromHere("../artifact-store/src/index.ts"),
      "@adriane-ai/search": fromHere("../search/src/index.ts"),
      "@adriane-ai/memory-store": fromHere("../memory-store/src/index.ts"),
      // The deprecated DSL compilers, now re-exported through the door for browser-safe YAML compile.
      "@adriane-ai/graph-adriane": fromHere("../graph-adriane/src/index.ts"),
      "@adriane-ai/lang-adriane": fromHere("../lang-adriane/src/index.ts")
    }
  }
});
