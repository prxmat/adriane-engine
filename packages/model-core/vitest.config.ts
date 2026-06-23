import { defineConfig } from "vitest/config";

// A local config so the per-package `vitest run` (turbo `pnpm test`) uses it rather than
// resolving the root vitest workspace. model-core's tests only need a node environment.
export default defineConfig({
  test: { environment: "node" }
});
