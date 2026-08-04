#!/usr/bin/env bun
/**
 * Build the @mediaway/browser wasm pkg deterministically.
 *
 * wasm-pack's `--out-dir` resolution base changed across versions (0.13+
 * resolves relative paths against the crate manifest dir, not the invoking
 * cwd), so pass an absolute out-dir and run the size pass afterwards.
 */

import { $ } from "bun";
import { join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const crate = join(root, "crates", "iso-bmff-wasm");
const outDir = join(root, "bindings", "browser", "packages", "browser", "pkg");

await $`wasm-pack build ${crate} --target web --out-dir ${outDir} --release`;
await $`bun ${join(root, "tools", "scripts", "optimize-wasm.ts")} ${join(outDir, "iso_bmff_wasm_bg.wasm")}`;
