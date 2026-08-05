#!/usr/bin/env bun
/**
 * cbindgen wrapper for `mediaway-ffi` — docs/adr/0016-cbindgen-ffi-headers.md.
 *
 * Usage:
 *   bun tools/scripts/cbindgen-headers.ts generate [--output <path>]
 *   bun tools/scripts/cbindgen-headers.ts verify <path-to-committed-header>
 *
 * `generate` runs the `cbindgen` CLI against `crates/mediaway-ffi` using its
 * `cbindgen.toml`, writing the result to `<path>` (default:
 * `target/cbindgen/mediaway_ffi.generated.h`). This is NOT yet the crate's real,
 * shipped `include/mediaway/{common,container,device,pipeline}.h` headers — those
 * stay hand-written until each is individually migrated and hardware-re-verified
 * (tracked per-header, ADR-0016 §4, not gated on this script existing).
 *
 * `verify` regenerates into a scratch temp file and diffs it byte-for-byte against
 * the committed file at `<path>`, exiting non-zero on mismatch — the CI drift gate
 * (ADR-0016 §3). Only meaningful once a real header at that path is itself
 * cbindgen-generated; do not wire this into CI against a still-hand-written header.
 *
 * Requires the `cbindgen` CLI on PATH (`cargo install cbindgen --locked`). Per
 * ADR-0016 §2, generation is an explicit command — never `build.rs`.
 */

import { $ } from "bun";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const configPath = join(root, "crates", "mediaway-ffi", "cbindgen.toml");
const defaultOutput = join(root, "target", "cbindgen", "mediaway_ffi.generated.h");

async function runCbindgen(outputPath: string): Promise<void> {
  mkdirSync(dirname(outputPath), { recursive: true });
  await $`cbindgen --crate mediaway-ffi -c ${configPath} -o ${outputPath}`.cwd(root);
}

const mode = process.argv[2];

if (mode === "generate") {
  const outFlagIndex = process.argv.indexOf("--output");
  const output = outFlagIndex !== -1 ? process.argv[outFlagIndex + 1] : defaultOutput;
  await runCbindgen(output);
  console.log(`generated: ${output}`);
} else if (mode === "verify") {
  const target = process.argv[3];
  if (!target) {
    console.error("usage: cbindgen-headers.ts verify <path-to-committed-header>");
    process.exit(1);
  }
  if (!existsSync(target)) {
    console.error(`missing committed header: ${target}`);
    process.exit(1);
  }
  const scratch = join(root, "target", "cbindgen", "verify.h");
  await runCbindgen(scratch);
  const committed = readFileSync(target, "utf8");
  const fresh = readFileSync(scratch, "utf8");
  rmSync(scratch);
  if (committed !== fresh) {
    console.error(`drift detected: ${target} does not match freshly generated output`);
    console.error(`run: bun tools/scripts/cbindgen-headers.ts generate --output ${target}`);
    process.exit(1);
  }
  console.log(`verified: ${target} matches generated output`);
} else {
  console.error("usage: cbindgen-headers.ts <generate|verify> [...]");
  process.exit(1);
}
