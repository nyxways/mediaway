#!/usr/bin/env bun
/**
 * Build all @mediaway/* Node packages for publishing: native DLL bundle +
 * TypeScript dist for each of the four packages.
 *
 * Usage:
 *   bun tools/scripts/build-node-packages.ts [--release]
 *
 * Equivalent to running `bun tools/scripts/copy-native-dlls.ts` followed by
 * `tsc -p tsconfig.json` in packages/{ffi,container,device,encoder}. Wired as
 * `npm run build` in bindings/nodejs.
 */

import { $ } from "bun";
import { join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const nodejs = join(root, "bindings", "nodejs");
const release = process.argv.includes("--release");

await $`bun ${join(root, "tools", "scripts", "copy-native-dlls.ts")}${release ? " --release" : ""}`;

for (const pkg of ["ffi", "container", "device", "encoder"]) {
  const dir = join(nodejs, "packages", pkg);
  await $`npx tsc -p tsconfig.json`.cwd(dir);
  console.log(`built @mediaway/${pkg} dist`);
}
