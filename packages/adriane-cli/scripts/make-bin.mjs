import { chmodSync, copyFileSync, existsSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(process.cwd());
const builtBin = resolve(root, "dist", "packages", "adriane-cli", "bin", "adriane.js");
const targetDir = resolve(root, "bin");
const target = resolve(targetDir, "adriane.js");

if (existsSync(builtBin)) {
  mkdirSync(targetDir, { recursive: true });
  copyFileSync(builtBin, target);
  chmodSync(target, 0o755);
}
