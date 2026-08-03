#!/usr/bin/env bun
/**
 * Optimize the browser wasm binary for size with binaryen (wasm-opt).
 *
 * Usage:
 *   bun tools/scripts/optimize-wasm.ts <wasm-file> [<wasm-file>...]
 *
 * wasm-pack's built-in wasm-opt integration is disabled for iso-bmff-wasm
 * (it auto-downloads binaryen and is flaky on Windows) — this script is the
 * deterministic replacement: pinned binaryen version, explicit `-Oz` flags,
 * in-place rewrite, before/after size report. Runs from the browser package
 * prepack (`npm run build:wasm`).
 *
 * binaryen is cached under local/binaryen/ (gitignored) and downloaded only
 * when missing — CI runners fetch it once per run.
 */

import { $ } from "bun";
import { existsSync, mkdirSync, statSync } from "node:fs";
import { join } from "node:path";

const BINARYEN_VERSION = "version_116";
const BINARYEN_TAG = "version_116";
const BINARYEN_URL = `https://github.com/WebAssembly/binaryen/releases/download/${BINARYEN_TAG}/binaryen-${BINARYEN_VERSION}-x86_64-windows.tar.gz`;
const BINARYEN_DIR = join(import.meta.dir, "..", "..", "local", "binaryen");
const WASM_OPT = join(BINARYEN_DIR, `binaryen-${BINARYEN_VERSION}`, "bin", "wasm-opt.exe");

function findWasmOpt(): string {
  if (existsSync(WASM_OPT)) return WASM_OPT;
  // A system wasm-opt (e.g. installed via binaryen) is fine too.
  const sys = Bun.which("wasm-opt");
  if (sys) return sys;
  return WASM_OPT;
}

async function ensureBinaryen(): Promise<void> {
  if (existsSync(WASM_OPT)) return;
  console.log("downloading binaryen", BINARYEN_VERSION, "-> local/binaryen/");
  mkdirSync(BINARYEN_DIR, { recursive: true });
  const tarball = join(BINARYEN_DIR, "binaryen.tar.gz");
  await $`curl -sL -o ${tarball} ${BINARYEN_URL}`;
  await $`tar -xzf ${tarball}`.cwd(BINARYEN_DIR);
}

const inputs = process.argv.slice(2);
if (inputs.length === 0) {
  console.error("usage: bun tools/scripts/optimize-wasm.ts <wasm-file> [<wasm-file>...]");
  process.exit(2);
}

await ensureBinaryen();
const wasmOpt = findWasmOpt();

let totalBefore = 0;
let totalAfter = 0;
for (const file of inputs) {
  if (!existsSync(file)) {
    console.error(`missing wasm file: ${file}`);
    process.exit(1);
  }
  const before = statSync(file).size;
  totalBefore += before;
  // -Oz: aggressively optimize for size; --strip-debug drops DWARF/name
  // sections (export names are preserved — the wasm-bindgen glue needs them).
  // --enable-*: the rustc output uses bulk memory / reference types / etc.
  await $`${wasmOpt} -Oz --strip-debug --vacuum --dce --enable-bulk-memory --enable-reference-types --enable-mutable-globals --enable-nontrapping-float-to-int --enable-sign-ext ${file} -o ${file}`;
  const after = statSync(file).size;
  totalAfter += after;
  console.log(`wasm-opt -Oz: ${file} ${before} -> ${after} bytes (-${Math.round((1 - after / before) * 100)}%)`);
}

if (inputs.length > 1) {
  console.log(`total: ${totalBefore} -> ${totalAfter} bytes (-${Math.round((1 - totalAfter / totalBefore) * 100)}%)`);
}
