#!/usr/bin/env bun
/**
 * CI affected-crate analysis — which workspace crates a change touches,
 * transitively (dependents included). Bun-based; the dependency graph comes
 * from `cargo metadata` (authoritative, handles workspace inheritance and
 * target-gated deps), the reachability logic is ours.
 *
 * Reverse-graph closure over ALL dependency kinds (normal + build + dev) —
 * changing a crate can break its dependents' builds or tests, so every
 * transitive dependent is included, not just direct ones.
 *
 * Usage:
 *   bun tools/scripts/ci-affected.ts --base origin/main      # diff merge-base..HEAD
 *   bun tools/scripts/ci-affected.ts --files "crates/iso-bmff/src/lib.rs Cargo.lock"
 *   bun tools/scripts/ci-affected.ts --one-line | --names | --json
 *
 * Output contract (for CI):
 *   NONE            no affected crates (skip clippy/test)
 *   ALL             everything affected (root manifest / lockfile / unknown rust)
 *   pkg1 pkg2 …     space-separated affected package names (one line)
 *
 * Wired into ci.yml: PRs run clippy + tests on the affected set; main pushes
 * keep the full workspace suite as the authoritative gate.
 */

import { spawnSync } from "node:child_process";
import { dirname, join, normalize, relative, sep } from "node:path";

interface Dep {
  name: string;
  kind: string | null; // null = normal, "dev", "build"
  source: string | null; // null = path dep
  target: string | null;
}

interface Package {
  name: string;
  manifestDir: string; // normalized forward-slash dir of manifest_path
  deps: Dep[];
}

interface Metadata {
  packages: Package[];
}

const ALL_TRIGGERS = [/^Cargo\.toml$/, /^Cargo\.lock$/, /^\.cargo\//, /^rust-toolchain(\.toml)?$/];

function usage(): never {
  console.error(`Usage: bun tools/scripts/ci-affected.ts [--base <ref> | --files "a b c"] [--one-line|--names|--json]
  --base <ref>   diff <merge-base(ref,HEAD)>..HEAD changed files (default: origin/main)
  --files "..."  override changed files (debug/testing)
  --one-line     space-separated names, or ALL / NONE   (default)
  --names        one name per line
  --json         {"affected": [...], "all": bool, "none": bool, "files": n}`);
  process.exit(2);
}

function parseArgs(argv: string[]): { base?: string; files?: string[]; mode: "one-line" | "names" | "json" } {
  const opts: { base?: string; files?: string[]; mode: "one-line" | "names" | "json" } = { mode: "one-line" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--base") opts.base = argv[++i];
    else if (a === "--files") opts.files = (argv[++i] ?? "").split(/\s+/).filter(Boolean);
    else if (a === "--one-line") opts.mode = "one-line";
    else if (a === "--names") opts.mode = "names";
    else if (a === "--json") opts.mode = "json";
    else usage();
  }
  return opts;
}

function run(cmd: string, args: string[]): string {
  const r = spawnSync(cmd, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) {
    console.error(`error: ${cmd} ${args.join(" ")} failed (${r.status ?? "signal"}):\n${(r.stderr ?? "").slice(0, 2000)}`);
    process.exit(1);
  }
  return r.stdout ?? "";
}

function changedFiles(opts: { base?: string; files?: string[] }): string[] {
  if (opts.files) return opts.files;
  const base = opts.base ?? "origin/main";
  // <base>...HEAD = diff from merge-base(base, HEAD) — what this branch adds.
  const out = run("git", ["diff", "--name-only", "--diff-filter=ACMRT", `${base}...HEAD`]);
  return out.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
}

function toSlash(p: string): string {
  return normalize(p).split(sep).join("/");
}

function loadMetadata(): Metadata {
  const raw = run("cargo", ["metadata", "--format-version=1"]);
  const meta = JSON.parse(raw) as {
    packages: Array<{
      name: string;
      manifest_path: string;
      dependencies: Array<{ name: string; kind: string | null; source: string | null; target: string | null }>;
    }>;
  };
  return {
    packages: meta.packages.map((p) => ({
      name: p.name,
      // Manifest dir made relative to the repo root (git paths are relative) —
      // CI runs the tool from the workspace root.
      manifestDir: toSlash(relative(process.cwd(), dirname(toSlash(p.manifest_path)))),
      deps: p.dependencies.map((d) => ({ name: d.name, kind: d.kind, source: d.source, target: d.target })),
    })),
  };
}

function main(): void {
  const opts = parseArgs(process.argv.slice(2));
  const files = changedFiles(opts);
  const meta = loadMetadata();
  const byDir = new Map<string, string>();
  for (const p of meta.packages) byDir.set(p.manifestDir, p.name);

  // --- map changed files to packages --------------------------------------
  const changedPkgs = new Set<string>();
  let all = false;
  for (const f of files) {
    const path = toSlash(f);
    if (ALL_TRIGGERS.some((re) => re.test(path))) {
      all = true;
      break;
    }
    // Longest matching package dir wins (e.g. crates/iso-bmff vs crates/iso-bmff-wasm).
    let best: string | undefined;
    let bestLen = -1;
    for (const [dir, name] of byDir) {
      if (path === dir || path.startsWith(dir + "/")) {
        if (dir.length > bestLen) {
          best = name;
          bestLen = dir.length;
        }
      }
    }
    if (best) {
      changedPkgs.add(best);
    } else if (path.startsWith("crates/")) {
      // Rust tree change that maps to no known package (e.g. deleted crate) —
      // be conservative: treat as workspace-wide.
      all = true;
      break;
    }
    // Everything else (docs/, .github/, bindings/, tools/scripts, local/, …)
    // is not Rust workspace code — ignore.
  }

  // --- reverse graph: dependent -> set of crates it depends on ------------
  const byName = new Map<string, Package>();
  for (const p of meta.packages) byName.set(p.name, p);
  // reverse edges over ALL kinds (normal + build + dev), target-gated included
  const dependents = new Map<string, Set<string>>();
  for (const p of meta.packages) {
    for (const d of p.deps) {
      if (!byName.has(d.name)) continue; // registry dep
      if (!dependents.has(d.name)) dependents.set(d.name, new Set());
      dependents.get(d.name)!.add(p.name);
    }
  }

  // --- BFS closure over dependents ----------------------------------------
  const affected = new Set<string>(changedPkgs);
  const queue = [...changedPkgs];
  while (queue.length > 0) {
    const cur = queue.pop()!;
    for (const dep of dependents.get(cur) ?? []) {
      if (!affected.has(dep)) {
        affected.add(dep);
        queue.push(dep);
      }
    }
  }

  // --- output ---------------------------------------------------------------
  if (all) {
    if (opts.mode === "json") {
      console.log(JSON.stringify({ affected: [], all: true, none: false, files: files.length }));
    } else {
      console.log("ALL");
    }
    return;
  }
  if (affected.size === 0) {
    if (opts.mode === "json") {
      console.log(JSON.stringify({ affected: [], all: false, none: true, files: files.length }));
    } else {
      console.log("NONE");
    }
    return;
  }
  const names = [...affected].sort();
  if (opts.mode === "json") {
    console.log(JSON.stringify({ affected: names, all: false, none: false, files: files.length }));
  } else if (opts.mode === "names") {
    console.log(names.join("\n"));
  } else {
    console.log(names.join(" "));
  }
}

main();
