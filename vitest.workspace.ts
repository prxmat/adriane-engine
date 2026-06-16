import { defineWorkspace } from "vitest/config";

// Aggregate every workspace's vitest project so a single root
// `vitest run --coverage` collects coverage across all engine packages.
export default defineWorkspace(["packages/*", "plugin/*"]);
