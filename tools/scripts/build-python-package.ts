#!/usr/bin/env bun
/**
 * Build the Python wheel for the `mediaway` package: native DLL bundle +
 * `python -m build` (falls back to `pip wheel` when the `build` package is
 * missing).
 *
 * Usage:
 *   bun tools/scripts/build-python-package.ts [--release]
 *
 * Output: bindings/python/dist/mediaway-*.whl
 */

import { $ } from "bun";
import { join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const pythonDir = join(root, "bindings", "python");
const release = process.argv.includes("--release");

await $`bun ${join(root, "tools", "scripts", "copy-native-dlls.ts")}${release ? " --release" : ""}`;

const buildCheck = await $`python -c "import build"`.cwd(pythonDir).quiet().nothrow();
if (buildCheck.exitCode === 0) {
  await $`python -m build`.cwd(pythonDir);
} else {
  await $`python -m pip wheel --no-deps -w dist .`.cwd(pythonDir);
}
console.log("wheel output: bindings/python/dist/");
