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
 * Cargo.lock changes are diffed rather than treated as a blanket trigger: the
 * lockfile's [[package]] entries at merge-base vs. HEAD are parsed (smol-toml) and
 * compared identity-by-identity (name+version, then checksum/dependencies for a
 * same-identity change) — only the package names that actually moved seed the
 * reverse-dependents BFS below, same as a changed source file would. Falls back to
 * ALL only when the diff can't be trusted (parse failure, lockfile format-version
 * bump, or `--files` debug mode with no git ref to diff against).
 *
 * Output contract (for CI):
 *   NONE            no affected crates (skip clippy/test)
 *   ALL             everything affected (root manifest / unparseable lockfile diff / unknown rust)
 *   pkg1 pkg2 …     space-separated affected package names (one line)
 *
 * Wired into ci.yml: PRs run clippy + tests on the affected set; main pushes
 * keep the full workspace suite as the authoritative gate.
 */

import { spawnSync } from "node:child_process";
import { dirname, normalize, relative, sep } from "node:path";
import { parse as parseToml } from "smol-toml";

interface Dep {
  name: string;
  kind: string | null; // null = normal, "dev", "build"
  source: string | null; // null = path dep
  target: string | null;
}

interface Package {
  id: string;
  name: string;
  manifestDir: string; // normalized forward-slash dir of manifest_path
  deps: Dep[];
}

interface Metadata {
  packages: Package[];
  workspaceMemberIds: Set<string>;
}

// Cargo.lock is deliberately NOT here — it gets a real diff (lockfileChangedNames)
// instead of a blanket trigger, since a single new dependency edge (the common case)
// only ever touches a handful of [[package]] entries, not the whole workspace.
const ALL_TRIGGERS = [/^Cargo\.toml$/, /\.cargo\//, /^rust-toolchain(\.toml)?$/, /^\.github\//];

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
      id: string;
      name: string;
      manifest_path: string;
      dependencies: Array<{ name: string; kind: string | null; source: string | null; target: string | null }>;
    }>;
    // Package IDs of the true workspace crates — `packages` also lists every
    // resolved external (registry/git) dependency, so this is the only
    // reliable way to tell "our crate" from "a dep that happens to live at a
    // path-like manifestDir" apart.
    workspace_members: string[];
  };
  return {
    packages: meta.packages.map((p) => ({
      id: p.id,
      name: p.name,
      // Manifest dir made relative to the repo root (git paths are relative) —
      // CI runs the tool from the workspace root.
      manifestDir: toSlash(relative(process.cwd(), dirname(toSlash(p.manifest_path)))),
      deps: p.dependencies.map((d) => ({ name: d.name, kind: d.kind, source: d.source, target: d.target })),
    })),
    workspaceMemberIds: new Set(meta.workspace_members),
  };
}

interface LockPackage {
  name: string;
  version: string;
  checksum?: string;
  dependencies?: string[];
}

interface LockDoc {
  formatVersion: unknown; // top-level `version = N`; undefined for an empty/missing lockfile
  packages: LockPackage[];
}

/** `git show <ref>:<path>`, but a missing path/ref (e.g. lockfile didn't exist yet at
 * the merge-base) returns `""` instead of failing the whole script — a real, expected
 * case (new lockfile), not an error. */
function gitShowOrEmpty(ref: string, path: string): string {
  const r = spawnSync("git", ["show", `${ref}:${path}`], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return r.status === 0 ? (r.stdout ?? "") : "";
}

/** Parses Cargo.lock's TOML into its `[[package]]` array. `null` means "could not be
 * trusted" (malformed) — the caller falls back to ALL rather than guessing. */
function parseLockPackages(src: string): LockDoc | null {
  if (!src.trim()) return { formatVersion: undefined, packages: [] };
  try {
    const doc = parseToml(src) as { version?: unknown; package?: LockPackage[] };
    return { formatVersion: doc.version, packages: doc.package ?? [] };
  } catch {
    return null;
  }
}

function lockEntryKey(p: LockPackage): string {
  return `${p.name}@${p.version}`;
}

/**
 * Diffs Cargo.lock's [[package]] entries between merge-base(base, HEAD) and HEAD,
 * returning the set of package NAMES (workspace or external — the reverse-dependents
 * BFS in `main` climbs from either) whose lock entry was added, removed, or changed
 * (version, checksum, or dependency list — the last one is what catches a workspace
 * crate gaining a new dependency edge with no version bump, this tool's original
 * motivating case).
 *
 * Returns the literal string "ALL" when the diff can't be trusted: a parse failure on
 * either side, or the lockfile format version itself changing (`version = N` in
 * Cargo.lock's header) — a format change could mean this function's own field
 * assumptions no longer hold, so it defers to the old conservative behavior rather
 * than silently under-reporting.
 */
function lockfileChangedNames(base: string): Set<string> | "ALL" {
  const mergeBase = run("git", ["merge-base", base, "HEAD"]).trim();
  const oldDoc = parseLockPackages(gitShowOrEmpty(mergeBase, "Cargo.lock"));
  const newDoc = parseLockPackages(gitShowOrEmpty("HEAD", "Cargo.lock"));
  if (!oldDoc || !newDoc) return "ALL";
  if (oldDoc.formatVersion !== undefined && newDoc.formatVersion !== undefined && oldDoc.formatVersion !== newDoc.formatVersion) {
    return "ALL";
  }

  const oldByKey = new Map(oldDoc.packages.map((p) => [lockEntryKey(p), p]));
  const newByKey = new Map(newDoc.packages.map((p) => [lockEntryKey(p), p]));
  const changed = new Set<string>();

  for (const [key, p] of newByKey) {
    const prev = oldByKey.get(key);
    if (!prev) {
      changed.add(p.name); // new identity: added package, or a version/source bump
      continue;
    }
    const sameChecksum = (prev.checksum ?? null) === (p.checksum ?? null);
    const sameDeps = JSON.stringify(prev.dependencies ?? []) === JSON.stringify(p.dependencies ?? []);
    if (!sameChecksum || !sameDeps) changed.add(p.name);
  }
  for (const [key, p] of oldByKey) {
    if (!newByKey.has(key)) changed.add(p.name); // identity removed at this version
  }
  return changed;
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
    if (path === "Cargo.lock") continue; // handled separately below (real diff, not a blanket trigger)
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

  // --- Cargo.lock: diff package identities instead of a blanket trigger ---
  if (!all && files.includes("Cargo.lock")) {
    if (opts.files) {
      // --files debug/testing mode has no git ref to diff the lockfile's old side
      // against — keep the old conservative behavior rather than guessing.
      all = true;
    } else {
      const seed = lockfileChangedNames(opts.base ?? "origin/main");
      if (seed === "ALL") {
        all = true;
      } else {
        for (const name of seed) changedPkgs.add(name);
      }
    }
  }

  // --- reverse graph: dependent -> set of crates it depends on ------------
  const byName = new Map<string, Package>();
  for (const p of meta.packages) byName.set(p.name, p);
  // Reverse edges over ALL kinds (normal + build + dev), target-gated included.
  // `byName.has(d.name)` spans the FULL resolved graph — cargo metadata's `packages`
  // already lists every external crate too, not just workspace members — so this
  // only actually skips a dep name that's declared but never resolved (e.g. an
  // optional/platform-gated dep no active feature set pulls in).
  const dependents = new Map<string, Set<string>>();
  for (const p of meta.packages) {
    for (const d of p.deps) {
      if (!byName.has(d.name)) continue;
      if (!dependents.has(d.name)) dependents.set(d.name, new Set());
      dependents.get(d.name)!.add(p.name);
    }
  }

  // --- BFS closure over dependents ----------------------------------------
  // changedPkgs may now include external crate names (from the lockfile diff above,
  // e.g. a transitive registry crate's version bump) — the BFS climbs through them
  // the same way it climbs through a changed workspace crate; the workspace-only
  // filter happens at output time below.
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
  // Workspace membership comes from cargo metadata's `workspace_members`, not
  // from "has a manifestDir" — `byDir` also maps every external registry/git
  // dependency to a (repo-external) manifestDir, which previously leaked
  // non-workspace crate names (e.g. objc2-audio-toolbox) into the affected
  // set passed to `cargo clippy -p <name>`.
  const workspaceNames = new Set(meta.packages.filter((p) => meta.workspaceMemberIds.has(p.id)).map((p) => p.name));
  const workspaceAffected = new Set([...affected].filter((n) => workspaceNames.has(n)));

  // --- output ---------------------------------------------------------------
  if (all) {
    if (opts.mode === "json") {
      console.log(JSON.stringify({ affected: [], all: true, none: false, files: files.length }));
    } else {
      console.log("ALL");
    }
    return;
  }
  if (workspaceAffected.size === 0) {
    if (opts.mode === "json") {
      console.log(JSON.stringify({ affected: [], all: false, none: true, files: files.length }));
    } else {
      console.log("NONE");
    }
    return;
  }
  const names = [...workspaceAffected].sort();
  if (opts.mode === "json") {
    console.log(JSON.stringify({ affected: names, all: false, none: false, files: files.length }));
  } else if (opts.mode === "names") {
    console.log(names.join("\n"));
  } else {
    console.log(names.join(" "));
  }
}

main();
