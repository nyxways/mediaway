#!/usr/bin/env bun
/**
 * Build the native `-ffi` cdylibs and copy them into @mediaway/ffi's `native/`
 * directory (the npm distribution's DLL bundle).
 *
 * Usage:
 *   bun tools/scripts/copy-native-dlls.ts [--release]
 *
 * Builds the three cdylibs for the GNU target (the bindings' verified build)
 * and copies them next to the package so `npm pack`/`publish` ships them and
 * the loader's `<package>/native` search path finds them. `--release` builds
 * with `--release` and copies from target/release instead of debug.
 *
 * Set MEDIAWAY_SKIP_CARGO_BUILD=1 to skip the cargo build and only stage the
 * DLLs that already exist (the release workflow prebuilds them once in its
 * `native-assets` job and downloads them as an artifact — see
 * .github/workflows/release.yml).
 */

import { $ } from "bun";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const release = process.argv.includes("--release");
const skipBuild = process.env.MEDIAWAY_SKIP_CARGO_BUILD === "1";
const profile = release ? "release" : "debug";
const target = "x86_64-pc-windows-gnu";
const cargoTargetDir = join(root, "target");
process.env.CARGO_TARGET_DIR = cargoTargetDir;

const crates = ["mediaway-container-ffi", "mediaway-pipeline-ffi", "mediaway-device-ffi"];
const dlls = ["mediaway_container_ffi.dll", "mediaway_pipeline_ffi.dll", "mediaway_device_ffi.dll"];

// Both npm and PyPI distributions bundle the DLLs next to their loader:
//   @mediaway/ffi/native/              (node, koffi absolute-path load)
//   mediaway/_native/                  (python, ctypes absolute-path load)
// NuGet (bindings/csharp): staged under runtime/win-x64/native, packed into
// the nupkgs' runtimes/win-x64/native by Directory.Build.targets.
// C/C++ (bindings/native): staged under runtime/win-x64 for the CMake/CPack
// package (bindings/cpp/CMakeLists.txt).
const nativeDirs = [
  join(root, "bindings", "nodejs", "packages", "ffi", "native"),
  join(root, "bindings", "python", "mediaway", "_native"),
  join(root, "bindings", "csharp", "runtime", "win-x64", "native"),
  join(root, "bindings", "native", "runtime", "win-x64"),
];
for (const dir of nativeDirs) mkdirSync(dir, { recursive: true });

if (skipBuild) {
  console.log("MEDIAWAY_SKIP_CARGO_BUILD=1 — staging prebuilt DLLs (no cargo build)");
} else {
  const args = ["build", ...(release ? ["--release"] : []), "--target", target];
  for (const c of crates) args.push("-p", c);
  await $`cargo ${args}`.cwd(root);
}

for (const dll of dlls) {
  const src = join(cargoTargetDir, target, profile, dll);
  if (!existsSync(src)) {
    throw new Error(`missing built cdylib: ${src}`);
  }
  for (const dir of nativeDirs) {
    copyFileSync(src, join(dir, dll));
  }
  console.log(`copied ${dll} (${profile}) -> node ffi/native/ + python mediaway/_native/`);
}

// Import libs (lib*.dll.a) for C/C++ static linking — staged next to the C/C++
// DLLs only; the node/python loaders never link.
for (const name of ["mediaway_container_ffi", "mediaway_pipeline_ffi", "mediaway_device_ffi"]) {
  const src = join(cargoTargetDir, target, profile, `lib${name}.dll.a`);
  if (existsSync(src)) {
    copyFileSync(src, join(nativeDirs[3], `lib${name}.dll.a`));
  }
}
console.log("copied import libs -> bindings/native/runtime/win-x64");
