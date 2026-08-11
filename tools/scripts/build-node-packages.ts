#!/usr/bin/env bun
/**
 * Build all @mediaway/* Node packages for publishing: native DLL bundle +
 * TypeScript dist for each of the five packages.
 *
 * Usage:
 *   bun tools/scripts/build-node-packages.ts [--release]
 *
 * Equivalent to running `bun tools/scripts/copy-native-dlls.ts` followed by
 * `tsc -p tsconfig.json` in packages/{ffi,container,device,decoder,encoder}
 * (device before decoder/encoder — encoder depends on device for the
 * capture-to-encode bridge). Wired as `npm run build` in bindings/nodejs.
 */

import { $ } from "bun";
import { join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const nodejs = join(root, "bindings", "nodejs");
const release = process.argv.includes("--release");

const dllScript = join(root, "tools", "scripts", "copy-native-dlls.ts").replaceAll("\\", "/");
const dllArgs = [dllScript];
if (release) dllArgs.push("--release");
await $`bun ${dllArgs}`.quiet();

// Resolve tsc by absolute path: CI bun install may skip .bin shims on
// Windows (symlink-less runners) and npx then installs the fake tsc@2.0.4
// placeholder package. `bun <path>` runs the JS directly, no PATH/`.cmd`
// resolution involved.
const tsc = join(nodejs, "node_modules", "typescript", "bin", "tsc");
for (const pkg of ["ffi", "container", "device", "decoder", "encoder"]) {
  const dir = join(nodejs, "packages", pkg);
  await $`bun ${tsc} -p tsconfig.json`.cwd(dir);
  console.log(`built @mediaway/${pkg} dist`);
}
