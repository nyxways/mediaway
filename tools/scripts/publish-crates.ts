#!/usr/bin/env bun
/**
 * Publish the crates.io set in dependency order (ADR-0021 consolidation).
 *
 * Usage:
 *   bun tools/scripts/publish-crates.ts [--dry-run] [--list]
 *
 * The publishable set = workspace crates with `publish = true` (cargo metadata
 * reports them as `publish: null`; workspace default is `publish = false`).
 * Currently 19 crates: 9 freestanding cores (rtmp stays `publish = false` —
 * Proposed) + mediaway-common/container/sw/encoder/decoder/device/mediaway +
 * avcli/avprobe + vpl-sys. mediaway-ffi is `publish = false` until its C ABI
 * ships.
 *
 * crates.io requires dependencies to exist before dependents upload, so the
 * set is topologically sorted by workspace dependency edges (computed from
 * cargo metadata — no hand-maintained order list to drift).
 *
 * `--dry-run` validates `cargo package` for every crate without uploading
 * (no token needed). A real run requires CARGO_REGISTRY_TOKEN and stops on
 * the first failure — registries are one-shot per version; bump the workspace
 * version and re-run to recover.
 */

import { $ } from "bun";
import { join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const args = new Set(process.argv.slice(2));
const dryRun = args.has("--dry-run");
const listOnly = args.has("--list");

if (!dryRun && !process.env.CARGO_REGISTRY_TOKEN && !listOnly) {
  console.error("CARGO_REGISTRY_TOKEN is not set — crates.io publish needs it (use --dry-run to validate without it)");
  process.exit(2);
}

interface CargoDep {
  name: string;
}
interface CargoPackage {
  name: string;
  version: string;
  source: string | null;
  publish: string[] | null;
  dependencies: CargoDep[];
}

// cargo metadata is tool output with a stable schema — narrow it once here.
function parseMetadata(raw: string): { packages: CargoPackage[] } {
  const value: unknown = JSON.parse(raw);
  if (value !== null && typeof value === "object" && "packages" in value && Array.isArray(value.packages)) {
    return { packages: value.packages as CargoPackage[] }; // shape checked above
  }
  throw new Error("unexpected cargo metadata shape");
}

// 1. metadata -> publishable workspace crates.
const metaOut = await $`cargo metadata --format-version 1`.cwd(root).quiet();
const { packages } = parseMetadata(metaOut.stdout.toString());
const byName = new Map(packages.map((p) => [p.name, p]));
const pub = packages
  .filter((p) => p.source === null && p.publish === null)
  .map((p) => p.name);
const pubSet = new Set(pub);

// 2. Topological sort (Kahn) on edges between publishable crates.
const depsOf = (name: string) =>
  byName.get(name)!.dependencies.filter((d) => pubSet.has(d.name)).map((d) => d.name);const indegree = new Map(pub.map((n) => [n, depsOf(n).length]));
const dependents = new Map(pub.map((n) => [n, [] as string[]]));
for (const n of pub) {
  for (const d of depsOf(n)) dependents.get(d)!.push(n);
}
const queue = pub.filter((n) => indegree.get(n) === 0).sort();
const order: string[] = [];
while (queue.length) {
  const n = queue.shift()!;
  order.push(n);
  for (const dep of dependents.get(n)!) {
    indegree.set(dep, indegree.get(dep)! - 1);
    if (indegree.get(dep) === 0) queue.push(dep);
    queue.sort();
  }
}
if (order.length !== pub.length) {
  console.error("dependency cycle in publishable set — refusing to guess an order");
  process.exit(1);
}

console.log(`crates.io set (${order.length}): ${order.join(" ")}`);
if (listOnly) process.exit(0);

// 2b. Already-uploaded check: the sparse index is the ground truth for what
// exists on the registry (the crates.io API returns null for deleted-crate
// names, which the publish endpoint still reserves — see the owner-error
// handling below).
function indexPath(name: string): string {
  if (name.length === 1) return `1/${name}`;
  if (name.length === 2) return `2/${name}`;
  if (name.length === 3) return `3/${name[0]}/${name}`;
  return `${name.slice(0, 2)}/${name.slice(2, 4)}/${name}`;
}
async function uploaded(name: string, version: string): Promise<boolean> {
  const res = await fetch(`https://index.crates.io/${indexPath(name)}`);
  if (!res.ok) return false;
  const text = await res.text();
  return text.split("\n").some((line) => line.includes(`"name":"${name}"`) && line.includes(`"vers":"${version}"`));
}

// 3. Publish in order.
const dryRunPassed = new Set<string>();
const blocked: string[] = [];
for (const name of order) {
  const pkg = byName.get(name)!;
  // Skip versions already on the registry (re-runs after a partial publish).
  if (!dryRun && (await uploaded(name, pkg.version))) {
    console.log(`already uploaded: ${name} ${pkg.version} — skip`);
    continue;
  }
  const cmd = dryRun ? ["publish", "-p", name, "--dry-run", "--allow-dirty"] : ["publish", "-p", name, "--allow-dirty"];
  console.log(`\n=== ${dryRun ? "checking" : "publishing"} ${name} ===`);
  const res = await $`cargo ${cmd}`.cwd(root).nothrow();
  if (res.exitCode !== 0) {
    // Dry-run still resolves the dep closure against the real crates.io index,
    // so a crate whose in-set deps only passed their dry-run in this run
    // reports "no matching package named X found". That is the expected
    // first-release gap, not a packaging error — log and continue.
    const out = res.stdout.toString() + res.stderr.toString();
    const missing = out.match(/no matching package named `([^`]+)` found/);
    if (dryRun && missing && dryRunPassed.has(missing[1])) {
      console.log(`expected dry-run gap: ${name} needs ${missing[1]} (only dry-run-published so far)`);
      dryRunPassed.add(name);
      continue;
    }
    // A name reserved by a deleted crate (visible neither in the index nor
    // the API) fails with the owner error — record and keep going so one
    // blocked name does not stall the whole batch.
    if (!dryRun && out.includes("this crate exists but you don't seem to be an owner")) {
      console.error(`BLOCKED: ${name} — name reserved on crates.io (deleted crate); rename and re-run`);
      blocked.push(name);
      continue;
    }
    console.error(`FAILED: ${name} (exit ${res.exitCode}) — bump the workspace version and re-run; half-published sets must be cleaned manually`);
    console.error(out.slice(-1500));
    process.exit(1);
  }
  dryRunPassed.add(name);
  console.log(`ok: ${name}`);
}
if (blocked.length > 0) {
  console.error(`\nblocked by reserved names: ${blocked.join(", ")} — rename these (collision policy) and re-run`);
  process.exit(1);
}
console.log(`\n${dryRun ? "dry-run" : "publish"} complete: ${order.length} crates`);
