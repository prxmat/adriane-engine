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
      // import the public `@adriane/graph-sdk` entry) and their tests exercise current
      // source instead of a possibly-stale `dist/`.
      "@adriane/graph-sdk": fromHere("./src/index.ts"),
      "@adriane/graph-core": fromHere("../graph-core/src/index.ts"),
      "@adriane/graph-runtime": fromHere("../graph-runtime/src/index.ts"),
      "@adriane/agents-core": fromHere("../agents-core/src/index.ts"),
      "@adriane/llm-gateway": fromHere("../llm-gateway/src/index.ts")
    }
  }
});
