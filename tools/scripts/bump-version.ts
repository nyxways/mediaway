#!/usr/bin/env bun
/**
 * Bump the Mediaway workspace version (root Cargo.toml [workspace.package]
 * version) and keep the internal lockstep dependency pins in sync.
 *
 * Crates that track the workspace version (`version.workspace = true`, per
 * ADR-0021's versioning addendum) are referenced in [workspace.dependencies]
 * with a `version = "X.Y"` requirement marked by a trailing
 * `# lockstep with workspace version` comment. That requirement only needs
 * to change on a minor/major bump (Cargo's default caret match already
 * covers patch releases within the same X.Y line), so this script recomputes
 * it from the new version and rewrites only the entries that actually moved.
 *
 * Freestanding unprefixed cores (iso-bmff, ebml-webm, flv-core, ...) publish
 * on their own cadence and are left untouched — they have no such marker.
 *
 * Usage:
 *   bun tools/scripts/bump-version.ts <new-version>   # e.g. 0.1.4, 0.2.0
 *
 * After running: `cargo check --workspace` to refresh Cargo.lock, then
 * `/release-notes <version>` to finalize the release note.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)$/;
const LOCKSTEP_LINE_RE = /^(.*version = ")(\d+(?:\.\d+)?)(" \}.*# lockstep with workspace version.*)$/;
const WORKSPACE_VERSION_RE = /^(version = ")(\d+\.\d+\.\d+)(")$/m;

function usage(): never {
  console.error("Usage: bun tools/scripts/bump-version.ts <new-version>   e.g. 0.1.4, 0.2.0");
  process.exit(2);
}

function requirementFor(version: string): string {
  const m = SEMVER_RE.exec(version)!;
  const [, major, minor] = m;
  // Pre-1.0: pin major.minor (Cargo's caret match covers patch releases).
  // Post-1.0: pin major only (breaking changes bump major by convention).
  return major === "0" ? `${major}.${minor}` : major;
}

function main(): void {
  const newVersion = process.argv[2];
  if (!newVersion || !SEMVER_RE.test(newVersion)) usage();

  const cargoTomlPath = join(import.meta.dir, "..", "..", "Cargo.toml");
  const original = readFileSync(cargoTomlPath, "utf8");
  const lines = original.split("\n");

  const workspaceIdx = lines.findIndex((l) => l.trim() === "[workspace.package]");
  if (workspaceIdx === -1) {
    console.error("error: [workspace.package] section not found in root Cargo.toml");
    process.exit(1);
  }
  const versionIdx = lines.findIndex((l, i) => i > workspaceIdx && WORKSPACE_VERSION_RE.test(l));
  if (versionIdx === -1) {
    console.error("error: version field not found under [workspace.package]");
    process.exit(1);
  }
  const currentVersion = WORKSPACE_VERSION_RE.exec(lines[versionIdx])![2];
  if (currentVersion === newVersion) {
    console.error(`error: workspace version is already ${newVersion}`);
    process.exit(1);
  }
  lines[versionIdx] = lines[versionIdx].replace(WORKSPACE_VERSION_RE, `$1${newVersion}$3`);
  console.log(`[workspace.package] version: ${currentVersion} -> ${newVersion}`);

  const newReq = requirementFor(newVersion);
  let depChanges = 0;
  for (let i = 0; i < lines.length; i++) {
    const m = LOCKSTEP_LINE_RE.exec(lines[i]);
    if (!m) continue;
    const [, prefix, oldReq, suffix] = m;
    if (oldReq === newReq) continue;
    lines[i] = `${prefix}${newReq}${suffix}`;
    depChanges++;
    console.log(`  lockstep dependency pin: ${oldReq} -> ${newReq} (${lines[i].trim().split(" ")[0]})`);
  }
  if (depChanges === 0) {
    console.log(`  lockstep dependency pins unchanged (still ${newReq})`);
  }

  writeFileSync(cargoTomlPath, lines.join("\n"));
  console.log("\nNext: cargo check --workspace   (refreshes Cargo.lock)");
  console.log("Then: /release-notes " + newVersion);
}

main();
