#!/usr/bin/env bun
/** Build wasm32 packages with wasm-bindgen for browser E2E. */

import { $ } from "bun";
import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "../../..");
const pkgRoot = join(import.meta.dir, "../pkg");
const target = "wasm32-unknown-unknown";
const profile = process.env.WASM_PROFILE ?? "release";
const flag = profile === "release" ? "--release" : "";
const cargoTargetDir = join(root, "target");
process.env.CARGO_TARGET_DIR = cargoTargetDir;

// mediaway-device is included: its `web` module's `#[wasm_bindgen]` exports
// (`open_user_media`, `device_selection_policy`, …) back the device-stream
// fixtures; the crate builds as a cdylib for wasm32 (see its Cargo.toml).
const crates = [
  "iso-bmff-wasm",
  "mediaway-encoder",
  "mediaway-decoder",
  "mediaway-device",
] as const;

mkdirSync(pkgRoot, { recursive: true });

await $`rustup target add ${target}`.cwd(root).nothrow();

const which = await $`wasm-bindgen --version`.cwd(root).quiet().nothrow();
if (which.exitCode !== 0) {
  console.log("Installing wasm-bindgen-cli…");
  await $`cargo install wasm-bindgen-cli --version 0.2.126 --locked`.cwd(root);
}

for (const crate of crates) {
  console.log(`Building ${crate}…`);
  await $`cargo build -p ${crate} --target ${target} ${flag}`.cwd(root);
  const outDir = join(pkgRoot, crate);
  mkdirSync(outDir, { recursive: true });
  const artifact = join(
    cargoTargetDir,
    target,
    profile,
    `${crate.replace(/-/g, "_")}.wasm`,
  );
  if (!existsSync(artifact)) {
    throw new Error(`Missing wasm artifact: ${artifact}`);
  }
  await $`wasm-bindgen --target web --out-dir ${outDir} ${artifact}`.cwd(root);
}

console.log("WASM packages ready under tools/e2e-web/pkg/");
