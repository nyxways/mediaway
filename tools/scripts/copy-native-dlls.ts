#!/usr/bin/env bun
/**
 * Build the native `-ffi` cdylib and copy it into every binding tree that
 * bundles a prebuilt native library (npm, PyPI, NuGet, C/C++).
 *
 * Usage:
 *   bun tools/scripts/copy-native-dlls.ts [--release] [--target <cargo-triple>]
 *
 * `--target` selects the platform to build for (default:
 * x86_64-pc-windows-gnu, this bindings' original verified build). Supported
 * triples and the tags they map to in each binding tree — see PLATFORMS
 * below. `--release` builds with `--release` and copies from target/release
 * instead of debug.
 *
 * Set MEDIAWAY_SKIP_CARGO_BUILD=1 to skip the cargo build and only stage the
 * native lib that already exists (the release workflow prebuilds it once per
 * platform in its `native-assets-*` jobs and downloads the artifacts — see
 * .github/workflows/release.yml).
 */

import { $ } from "bun";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";

// One entry per (OS, arch) this workspace ships a prebuilt native library
// for. `rid` is the .NET/NuGet runtime identifier, `npmTag` is the
// `os-cpu` suffix @mediaway/ffi-<npmTag> packages use (matches npm's own
// `os`/`cpu` package.json fields), `libName` is the cdylib filename Cargo
// produces for that target.
const PLATFORMS = {
  "x86_64-pc-windows-gnu": { rid: "win-x64", npmTag: "win32-x64", libName: "mediaway_ffi.dll" },
  "x86_64-unknown-linux-gnu": { rid: "linux-x64", npmTag: "linux-x64", libName: "libmediaway_ffi.so" },
  "x86_64-apple-darwin": { rid: "osx-x64", npmTag: "darwin-x64", libName: "libmediaway_ffi.dylib" },
  "aarch64-apple-darwin": { rid: "osx-arm64", npmTag: "darwin-arm64", libName: "libmediaway_ffi.dylib" },
} as const;

type Triple = keyof typeof PLATFORMS;

const root = join(import.meta.dir, "..", "..");
const release = process.argv.includes("--release");
const skipBuild = process.env.MEDIAWAY_SKIP_CARGO_BUILD === "1";
const profile = release ? "release" : "debug";

const targetFlagIndex = process.argv.indexOf("--target");
const target = (targetFlagIndex >= 0 ? process.argv[targetFlagIndex + 1] : "x86_64-pc-windows-gnu") as Triple;
const platform = PLATFORMS[target];
if (!platform) {
  throw new Error(`unknown --target ${target}; supported: ${Object.keys(PLATFORMS).join(", ")}`);
}
const { rid, npmTag, libName } = platform;

const cargoTargetDir = join(root, "target");
process.env.CARGO_TARGET_DIR = cargoTargetDir;

// Windows-only: Cargo also emits a GNU import lib (lib*.dll.a) for C/C++
// static linking. Linux/macOS consumers link the shared object directly, no
// import lib involved.
const isWindows = target === "x86_64-pc-windows-gnu";

// npm and PyPI distributions bundle the native lib next to their loader:
//   @mediaway/ffi/native/<npmTag>/     (node, koffi absolute-path load —
//     every platform's lib ships in the one package, see
//     bindings/nodejs/packages/ffi/src/index.ts's platformTag() resolution)
//   mediaway/_native/                  (python, ctypes absolute-path load —
//     one platform staged at a time; the PyPI job builds one wheel per
//     platform, re-running this script between builds)
// NuGet (bindings/csharp): staged under runtime/<rid>/native, packed into
// the nupkgs' runtimes/<rid>/native by Directory.Build.targets.
// C/C++ (bindings/native): staged under runtime/<rid> for the CMake/CPack
// package (bindings/cpp/CMakeLists.txt).
const nodeNativeDir = join(root, "bindings", "nodejs", "packages", "ffi", "native", npmTag);
const nativeDirs = [
  nodeNativeDir,
  join(root, "bindings", "python", "mediaway", "_native"),
  join(root, "bindings", "csharp", "runtime", rid, "native"),
  join(root, "bindings", "native", "runtime", rid),
];
for (const dir of nativeDirs) mkdirSync(dir, { recursive: true });

if (skipBuild) {
  console.log(`MEDIAWAY_SKIP_CARGO_BUILD=1 — staging prebuilt ${libName} (no cargo build)`);
} else {
  const args = ["build", ...(release ? ["--release"] : []), "--target", target, "-p", "mediaway-ffi"];
  await $`cargo ${args}`.cwd(root);
}

const src = join(cargoTargetDir, target, profile, libName);
if (!existsSync(src)) {
  // Skip-build + artifact flow (release.yml): the native-assets-* job already
  // staged the lib into nativeDirs and the publish jobs download it — a
  // missing target/ build dir is expected, not an error, as long as the
  // staged copy exists. No-op so packing prepack steps work in CI.
  const staged = nativeDirs.map((d) => join(d, libName)).filter(existsSync);
  if (staged.length > 0) {
    console.log(`already staged: ${libName} (${profile} build missing, keeping staged copy)`);
  } else {
    throw new Error(`missing built cdylib: ${src}`);
  }
} else {
  for (const dir of nativeDirs) {
    copyFileSync(src, join(dir, libName));
  }
  console.log(`copied ${libName} (${profile}, ${target}) -> ${rid} / ${npmTag} binding trees`);
}

// Import lib (lib*.dll.a) for C/C++ static linking — Windows only, staged
// next to the C/C++ DLL only; the node/python loaders never link.
if (isWindows) {
  const importSrc = join(cargoTargetDir, target, profile, "libmediaway_ffi.dll.a");
  if (existsSync(importSrc)) {
    copyFileSync(importSrc, join(nativeDirs[3], "libmediaway_ffi.dll.a"));
    console.log(`copied import lib -> bindings/native/runtime/${rid}`);
  }
}
