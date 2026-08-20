#!/usr/bin/env bun
/**
 * Build the Python wheel for the `mediaway` package: native lib bundle +
 * `python -m build` (falls back to `pip wheel` when the `build` package is
 * missing).
 *
 * Usage:
 *   bun tools/scripts/build-python-package.ts [--release] [--target <rid>]
 *
 * `--target` selects which platform's wheel to build (default: win-x64) —
 * one of win-x64/linux-x64/osx-x64/osx-arm64 (ADR-0024).
 *
 * With MEDIAWAY_SKIP_CARGO_BUILD unset, this rebuilds the cdylib from
 * source and cleans bindings/python/mediaway/_native/ first so a previous
 * platform's staged lib never leaks into this one. With
 * MEDIAWAY_SKIP_CARGO_BUILD=1 (the release.yml prebuilt-artifact flow), the
 * caller is responsible for staging exactly the right platform's lib into
 * _native/ before calling this script — it neither cleans nor re-copies,
 * since release.yml's own per-platform loop already did that from the
 * downloaded native-assets-* artifacts.
 *
 * Output: bindings/python/dist/mediaway-*.whl
 */

import { $ } from "bun";
import { join } from "node:path";
import { existsSync, readdirSync, rmSync } from "node:fs";

const RID_TO_TRIPLE: Record<string, string> = {
  "win-x64": "x86_64-pc-windows-gnu",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "osx-x64": "x86_64-apple-darwin",
  "osx-arm64": "aarch64-apple-darwin",
};

const root = join(import.meta.dir, "..", "..");
const pythonDir = join(root, "bindings", "python");
const release = process.argv.includes("--release");
const skipBuild = process.env.MEDIAWAY_SKIP_CARGO_BUILD === "1";

const targetFlagIndex = process.argv.indexOf("--target");
const rid = targetFlagIndex >= 0 ? process.argv[targetFlagIndex + 1] : "win-x64";
const triple = RID_TO_TRIPLE[rid];
if (!triple) {
  throw new Error(`unknown --target ${rid}; supported: ${Object.keys(RID_TO_TRIPLE).join(", ")}`);
}

if (!skipBuild) {
  const nativeDir = join(pythonDir, "mediaway", "_native");
  if (existsSync(nativeDir)) {
    for (const f of readdirSync(nativeDir)) rmSync(join(nativeDir, f), { force: true });
  }

  const dllScript = join(root, "tools", "scripts", "copy-native-dlls.ts").replaceAll("\\", "/");
  const dllArgs = [dllScript, "--target", triple];
  if (release) dllArgs.push("--release");
  await $`bun ${dllArgs}`.quiet();
}

// setuptools' own build/ staging dir persists native libs from a previous
// platform's build (it doesn't know _native/'s contents changed underneath
// it) — clean it every time, not just when we skip the cargo rebuild, or a
// wheel built later in a multi-platform loop (see the pypi job in
// release.yml) silently accumulates every earlier platform's lib too.
const buildStagingDir = join(pythonDir, "build");
if (existsSync(buildStagingDir)) {
  rmSync(buildStagingDir, { recursive: true, force: true });
}

const env = { ...process.env, MEDIAWAY_WHEEL_PLATFORM: rid };

// Check for the real PyPA build module from a cwd where the local
// bindings/python/build/ output dir cannot shadow it.
const buildCheck = await $`python -c "import build"`.cwd(root).quiet().nothrow();
if (buildCheck.exitCode === 0) {
  await $`python -m build`.cwd(pythonDir).env(env);
} else {
  await $`python -m pip wheel --no-deps -w dist .`.cwd(pythonDir).env(env);
}
console.log(`wheel output: bindings/python/dist/ (platform: ${rid})`);
