/**
 * Native library discovery + koffi load for @mediaway/ffi.
 *
 * Split out of index.ts (ADR-0024, multi-platform binding distribution)
 * once the added per-platform search-path logic pushed index.ts's struct/
 * function-binding definitions past the workspace's 1000-line source cap.
 */

import koffi from "koffi";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// ── Library discovery ──────────────────────────────────────────────────────────
// The cdylib is a Rust build artifact, not an installed system library.
// Search:
//   1. $MEDIAWAY_FFI_DIR
//   2. <this package>/native/<platform>-<arch>   (libs for every platform
//      bundled at pack time — the npm distribution; see § platform tag)
//   3. <repo root>/target/x86_64-pc-windows-gnu/debug   (GNU toolchain, dev runs)
//   4. <repo root>/target/debug                          (host/MSVC toolchain)
//   5. cwd
const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..", "..", "..");

/**
 * `os-arch` tag matching one of `native/<tag>`'s subdirectories (this
 * package bundles every platform's prebuilt lib directly — see
 * tools/scripts/copy-native-dlls.ts). Returns `undefined` for an
 * unsupported platform/arch pair rather than throwing, so callers can still
 * fall through to the other search dirs (e.g. `$MEDIAWAY_FFI_DIR` during
 * local development on an unlisted arch).
 */
function platformTag(): string | undefined {
  const os = process.platform === "win32" ? "win32" : process.platform === "linux" ? "linux"
    : process.platform === "darwin" ? "darwin" : undefined;
  const arch = process.arch === "x64" ? "x64" : process.arch === "arm64" ? "arm64" : undefined;
  return os && arch ? `${os}-${arch}` : undefined;
}

const tag = platformTag();
const searchDirs = [
  process.env.MEDIAWAY_FFI_DIR ?? "",
  tag ? path.resolve(here, "..", "native", tag) : "",
  path.join(repoRoot, "target", "x86_64-pc-windows-gnu", "debug"),
  path.join(repoRoot, "target", "debug"),
  process.cwd(),
];

export function findLibrary(name: string): string {
  for (const dir of searchDirs) {
    if (!dir) continue;
    const candidate = path.join(dir, name);
    if (fs.existsSync(candidate)) return candidate;
  }
  throw new Error(
    `cannot find ${name}; set $MEDIAWAY_FFI_DIR or build the -ffi crates`
  );
}

/** cdylib filename Cargo produces for this platform. */
function libraryFilename(): string {
  switch (process.platform) {
    case "win32":
      return "mediaway_ffi.dll";
    case "linux":
      return "libmediaway_ffi.so";
    case "darwin":
      return "libmediaway_ffi.dylib";
    default:
      throw new Error(`mediaway: unsupported platform ${process.platform}`);
  }
}

function load(name: string) {
  return koffi.load(findLibrary(name));
}

const libraryName = libraryFilename();
export const containerLib = load(libraryName);
export const pipelineLib = load(libraryName);
export const deviceLib = load(libraryName);
