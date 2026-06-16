import { defineConfig, globalIgnores } from "eslint/config";
import tsParser from "@typescript-eslint/parser";
import tsPlugin from "@typescript-eslint/eslint-plugin";

export default defineConfig([
  globalIgnores([
    "**/dist/**",
    "**/.next/**",
    "**/node_modules/**",
    "**/coverage/**",
    // Next.js generates triple-slash refs; ESLint forbids rewriting this file.
    "**/next-env.d.ts"
  ]),
  {
    files: ["**/*.ts", "**/*.tsx"],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: "latest",
        sourceType: "module"
      }
    },
    plugins: {
      "@typescript-eslint": tsPlugin
    },
    rules: {
      ...tsPlugin.configs.recommended.rules
    }
  }
]);
