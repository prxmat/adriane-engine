import { defineConfig } from "vitest/config";

// Root config: minimal v8 coverage settings, applied when running the workspace
// (see vitest.workspace.ts) via `pnpm test:coverage`.
export default defineConfig({
  test: {
    coverage: {
      provider: "v8",
      reporter: ["text", "json", "html"],
      reportsDirectory: "coverage"
    }
  }
});
