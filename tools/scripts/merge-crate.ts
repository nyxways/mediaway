#!/usr/bin/env bun
/**
 * merge-crate.ts — merge child crates into a parent crate as `#[cfg]`-gated modules.
 *
 * Migration tool for ADR-0021 workspace consolidation. Mechanical, reviewable,
 * history-preserving (git mv), dry-run by default:
 *
 *   bun tools/scripts/merge-crate.ts <spec.json>          # plan only
 *   bun tools/scripts/merge-crate.ts <spec.json> --apply  # execute
 *
 * Spec shape:
 *   {
 *     "crate": "mediaway-ffi",
 *     "dir": "crates/mediaway-ffi",          // destination (created if missing)
 *     "description": "…",
 *     "children": [
 *       { "name": "mediaway-container-ffi", "dir": "crates/mediaway-container-ffi", "module": "container" },
 *       …
 *     ]
 *   }
 *
 * What it does (each step is planned, then executed only with --apply):
 *   1. git mv each child's src -> <dir>/src/<module>; tests/benches likewise;
 *      include/* merged into <dir>/include/; adr -> <dir>/adr/<module>; docs ->
 *      <dir>/docs/<module>; README.md -> <dir>/docs/<module>/README.md; git rm
 *      the child Cargo.toml.
 *   2. Root workspace Cargo.toml: drop children from `members` and
 *      `[workspace.dependencies]`, add the parent (exact-line anchored).
 *   3. Rust rewrites (tree-sitter-rust targeted, byte-applied at node ranges)
 *      on moved src/tests files:
 *        - `mediaway_common_ffi::` -> `crate::common::`  (module != common)
 *        - `crate::` -> `crate::<module>::`  (never `crate::<module>::`/`crate::common::`)
 *        - integration tests: `<child>_ffi::` -> `<parent>_ffi::`
 *        - doc-link depth: `../../../docs/` -> `../../../../docs/`, `(adr/` -> `(../../adr/<module>/`
 *   4. Non-Rust reference rewrites across tracked bindings/tools/.github/docs
 *      files (word-boundary; tree-sitter-json/typescript where the file parses):
 *        - `mediaway-{container,common,device,pipeline}-ffi` -> `mediaway-ffi`
 *        - `mediaway_{container,device,pipeline}_ffi` -> `mediaway_ffi`  (cdylib/DLL names)
 *   5. Scaffold a draft parent Cargo.toml (union of child deps/features) and
 *      src/lib.rs (`pub mod <module>;`) — printed in dry-run, written with --apply,
 *      always review before building.
 *
 * TOML edits are exact-line anchored (not tree-sitter): the Cargo.toml entries
 * this tool touches are single-line, verbatim-matchable keys. Rust/TS/JSON use
 * tree-sitter. See tools/scripts/package.json deps (tree-sitter, tree-sitter-rust,
 * tree-sitter-json, tree-sitter-typescript).
 */

import Parser from "tree-sitter";
import RustLang from "tree-sitter-rust";
import JsonLang from "tree-sitter-json";
import { typescript as TsLang } from "tree-sitter-typescript";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, relative, dirname, basename } from "node:path";

const ROOT = process.cwd();

// ── args ────────────────────────────────────────────────────────────────────
const args = process.argv.slice(2);
const specPath = args.find((a) => !a.startsWith("--"));
const APPLY = args.includes("--apply");
if (!specPath) {
  console.error("usage: bun merge-crate.ts <spec.json> [--apply]");
  process.exit(2);
}
const spec = JSON.parse(readFileSync(specPath, "utf8"));

// ── helpers ─────────────────────────────────────────────────────────────────
const log = (s = "") => console.log(s);
const plan = []; // { what, detail } printed in order; executed in order
const edits = []; // { path, old, new } content edits (plan items reference them)
const movedOrig = new Map(); // moved target -> original path (dry-run reads originals)

function git(...cmd) {
  return execFileSync("git", cmd, { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] })
    .trim();
}

const toSlash = (p) => p.replace(/\\/g, "/");

function isTracked(p) {
  try {
    git("ls-files", "--error-unmatch", "--", p);
    return true;
  } catch {
    return false;
  }
}

function listFiles(dir) {
  const out = git("ls-files", dir).split("\n").filter(Boolean);
  return out.map(toSlash);
}

function queueEdit(path, old, next) {
  const cur = readFileSync(path, "utf8");
  if (!cur.includes(old)) throw new Error(`${path}: anchor not found: ${JSON.stringify(old.slice(0, 60))}`);
  edits.push({ path, old, next, count: cur.split(old).length - 1 });
}

// ── tree-sitter languages (native prebuilds, verified working under Bun) ────
const parser = new Parser();
parser.setLanguage(RustLang);
const jsonParser = new Parser();
jsonParser.setLanguage(JsonLang);
const tsParser = new Parser();
tsParser.setLanguage(TsLang);

// ── step 1: moves ───────────────────────────────────────────────────────────
const movedFiles = []; // relative paths that moved (for rewrite passes)
for (const child of spec.children) {
  for (const [sub, dest] of [
    ["src", join(spec.dir, "src", child.module)],
    ["tests", join(spec.dir, "tests", child.module)],
    ["benches", join(spec.dir, "benches", child.module)],
    ["adr", join(spec.dir, "adr", child.module)],
    ["docs", join(spec.dir, "docs", child.module)],
  ]) {
    const srcDir = join(child.dir, sub);
    if (!existsSync(srcDir) || listFiles(srcDir).length === 0) continue;
    plan.push({ what: `git mv ${srcDir}/ -> ${dest}/`, detail: `${listFiles(srcDir).length} files` });
    for (const f of listFiles(srcDir)) {
      const target = join(dest, relative(srcDir, f));
      movedFiles.push(toSlash(target));
      movedOrig.set(toSlash(target), toSlash(f));
      if (APPLY) {
        mkdirSync(dirname(target), { recursive: true });
        execFileSync("git", ["mv", "--", f, target], { cwd: ROOT, stdio: "ignore" });
      }
    }
  }
  // include/ merges file-by-file into the parent include/ tree
  const incDir = join(child.dir, "include");
  if (existsSync(incDir)) {
    for (const f of listFiles(incDir)) {
      const target = join(spec.dir, "include", relative(incDir, f));
      plan.push({ what: `git mv ${f} -> ${target}`, detail: "" });
      movedFiles.push(toSlash(target));
      movedOrig.set(toSlash(target), toSlash(f));
      if (APPLY) {
        mkdirSync(dirname(target), { recursive: true });
        execFileSync("git", ["mv", "--", f, target], { cwd: ROOT, stdio: "ignore" });
      }
    }
  }
  // module root file: src/<module>/lib.rs -> src/<module>/mod.rs (dir modules need mod.rs)
  {
    const lib = join(spec.dir, "src", child.module, "lib.rs");
    const mod = join(spec.dir, "src", child.module, "mod.rs");
    if (existsSync(lib)) {
      plan.push({ what: `git mv ${toSlash(lib)} -> ${toSlash(mod)}`, detail: "dir-module root" });
      if (APPLY) execFileSync("git", ["mv", "--", toSlash(lib), toSlash(mod)], { cwd: ROOT, stdio: "ignore" });
    }
  }
  const readme = join(child.dir, "README.md");
  if (existsSync(readme) && isTracked(readme)) {
    const target = join(spec.dir, "docs", child.module, "README.md");
    plan.push({ what: `git mv ${readme} -> ${target}`, detail: "" });
    movedFiles.push(toSlash(target));
    movedOrig.set(toSlash(target), toSlash(readme));
    if (APPLY) {
      mkdirSync(dirname(target), { recursive: true });
      execFileSync("git", ["mv", "--", readme, target], { cwd: ROOT, stdio: "ignore" });
    }
  }
  plan.push({ what: `git rm ${child.dir}/Cargo.toml`, detail: "deps merged into parent (see draft)" });
  if (APPLY && existsSync(join(child.dir, "Cargo.toml")))
    execFileSync("git", ["rm", "--", join(child.dir, "Cargo.toml")], { cwd: ROOT, stdio: "ignore" });
}

// ── step 2: root workspace manifest ─────────────────────────────────────────
// Idempotent: re-running after a partial apply skips already-done edits.
{
  const wc = readFileSync("Cargo.toml", "utf8");
  const childDirs = spec.children.map((c) => toSlash(c.dir));
  // members array entries are exactly `    "crates/…",` lines
  const membersRe = /^(\s*)"(crates\/[^"]+)",\s*$/gm;
  const memberLines = [];
  let m;
  while ((m = membersRe.exec(wc)) !== null) memberLines.push(m);
  const present = memberLines.filter((x) => childDirs.includes(x[2]));
  if (present.length !== spec.children.length) {
    log(`⚠ members: found ${present.length}/${spec.children.length} children in workspace members`);
  }
  // Queue the parent ADD before the removals: the add anchors on a line the
  // removals delete, so it must run first in the apply loop.
  const want = toSlash(spec.dir);
  if (!memberLines.some((x) => x[2] === want)) {
    const anchor = present[0] ?? memberLines.find((x) => x[2] < want);
    plan.push({ what: `Cargo.toml members: add "${want}"`, detail: "at first removed position" });
    queueEdit("Cargo.toml", anchor[0], `    "${want}",\n${anchor[0]}`);
  }
  for (const p of present) {
    plan.push({ what: `Cargo.toml members: remove "${p[2]}"`, detail: "" });
    queueEdit("Cargo.toml", p[0], "");
  }
  // workspace.dependencies entries: `mediaway-…-ffi = { path = "crates/…", version = "0.1.0" }`
  const parentDep = `${spec.crate} = { path = "${toSlash(spec.dir)}", version = "0.1.0" }`;
  if (!wc.includes(parentDep)) {
    const firstChild = spec.children.find((c) => wc.includes(`${c.name} = { path = "`));
    const anchor = firstChild && wc.match(new RegExp(`^${firstChild.name} = \{ path = "[^"]+", version = "[^"]+" \}`, "m"));
    plan.push({ what: `Cargo.toml [workspace.dependencies]: add ${parentDep}`, detail: "" });
    if (anchor) queueEdit("Cargo.toml", anchor[0], `${parentDep}\n${anchor[0]}`);
    else plan.push({ what: "Cargo.toml: add parent dep MANUALLY (no anchor)", detail: parentDep });
  }
  for (const child of spec.children) {
    const re = new RegExp(`^(${child.name} = \{ path = "[^"]+", version = "[^"]+" \}\s*)$`, "m");
    const hit = wc.match(re);
    if (!hit) {
      log(`⚠ [workspace.dependencies]: no entry for ${child.name} — check manually`);
      continue;
    }
    plan.push({ what: `Cargo.toml [workspace.dependencies]: remove ${child.name}`, detail: "" });
    queueEdit("Cargo.toml", hit[1], "");
  }
}

// ── step 3: Rust rewrites on moved files ────────────────────────────────────
function rustUsePaths(src, want) {
  // Return ranges of `use …;` declarations whose path contains `want` textually.
  const out = [];
  const t = parser.parse(src);
  const walk = (n) => {
    if (n.type === "use_declaration") {
      const txt = src.slice(n.startIndex, n.endIndex);
      if (txt.includes(want)) out.push([n.startIndex, n.endIndex]);
    }
    for (const c of n.namedChildren) walk(c);
  };
  walk(t.rootNode);
  return out;
}

for (const child of spec.children) {
  // Glob the destination dirs directly — idempotent across re-runs (files may
  // already be moved by an earlier apply; content rewrites must not skip them).
  const moduleFiles = git("ls-files", toSlash(join(spec.dir, "src", child.module)))
    .split("\n").filter(Boolean)
    .map(toSlash)
    .filter((f) => f.endsWith(".rs"))
    .concat(
      // dry-run: nothing moved yet — use the planned move list
      movedFiles.filter(
        (f) => f.startsWith(`${toSlash(spec.dir)}/src/${child.module}/`) && f.endsWith(".rs")
      )
    )
    .filter((v, i, a) => a.indexOf(v) === i);
  const parentUnderscore = spec.crate.replace(/-/g, "_");
  const childUnderscore = child.name.replace(/-/g, "_");
  for (const f of moduleFiles) {
    const srcPath = APPLY ? f : (movedOrig.get(f) ?? f);
    const orig = readFileSync(srcPath, "utf8");
    let s = orig;
    const rangeApply = (ranges, fn) => {
      // apply fn to each [start,end) slice, bottom-up (reverse order keeps offsets valid)
      for (const [a, b] of ranges.slice().reverse()) {
        const replaced = fn(s.slice(a, b));
        if (replaced !== s.slice(a, b)) s = s.slice(0, a) + replaced + s.slice(b);
      }
    };
    // (a) shared rlib references
    if (child.module !== "common") {
      const ranges = rustUsePaths(s, "mediaway_common_ffi");
      rangeApply(ranges, (seg) => seg.split("mediaway_common_ffi::").join("crate::common::"));
    }
    // (b) crate-root paths gain the module prefix (skip existing module/common refs)
    const skip = `(?:${child.module}::|common::)`;
    const re = new RegExp(`crate::(?!${skip})`, "g");
    s = s.replace(re, `crate::${child.module}::`);
    // (c) doc-link depth (moved files sit one level deeper)
    s = s.split("../../../docs/").join("../../../../docs/");
    if (child.module !== "common") {
      s = s.split(`(adr/`).join(`(../../adr/${child.module}/`).split(`[adr/`).join(`[../../adr/${child.module}/`);
    }
    if (s !== orig) {
      plan.push({ what: `rewrite ${f}`, detail: `${(orig.split(orig).length - 1)} edited (rust)` });
      edits.push({ path: f, old: orig, next: s, count: 1 });
    }
  }
  // integration tests reference the child crate by name
  const testFiles = git("ls-files", toSlash(spec.dir))
    .split("\n").filter(Boolean)
    .map(toSlash)
    .filter((f) => f.startsWith(`${toSlash(spec.dir)}/tests/`) && f.endsWith(".rs"))
    .concat(
      movedFiles.filter(
        (f) => f.startsWith(`${toSlash(spec.dir)}/tests/`) && f.endsWith(".rs")
      )
    )
    .filter((v, i, a) => a.indexOf(v) === i);
  for (const f of testFiles) {
    const srcPath = APPLY ? f : (movedOrig.get(f) ?? f);
    const orig = readFileSync(srcPath, "utf8");
    // integration tests referenced the child crate's root re-exports; in the
    // merged crate those live under the module path
    const s = orig.split(`${childUnderscore}::`).join(`${parentUnderscore}::${child.module}::`);
    if (s !== orig) {
      plan.push({ what: `rewrite ${f}`, detail: `crate ref ${childUnderscore} -> ${parentUnderscore}` });
      edits.push({ path: f, old: orig, next: s, count: 1 });
    }
  }
}

// ── step 4: non-Rust reference rewrites (tracked, non-build-output) ────────
const SKIP_DIRS = ["node_modules", "target", "dist", "build", "runtime", "pkg", ".git"];
// Historical records keep their original crate names (ADR-0021 amends them in
// place; the old names document the pre-consolidation design). Current-state
// docs (wiki, roadmap, runbooks) are rewritten. Specs are updated by hand.
const SKIP_PREFIXES = ["docs/adr/", "docs/spec/"];
function sweepable() {
  const roots = ["bindings", "tools", "docs", "examples"];
  const out = [];
  for (const r of roots) {
    if (!existsSync(r)) continue;
    for (const f of git("ls-files", r).split("\n").filter(Boolean)) {
      const s = toSlash(f);
      if (SKIP_DIRS.some((d) => s.split("/").includes(d))) continue;
      if (SKIP_PREFIXES.some((p) => s.startsWith(p))) continue;
      if (!/\.(ts|tsx|js|mjs|cjs|json|cs|csproj|props|py|yml|yaml|cmake|txt|md|bats|ps1)$/.test(s)) continue;
      out.push(s);
    }
  }
  return out;
}
const nameMaps = [
  [/\bmediaway-(?:container|common|device|pipeline)-ffi\b/g, "mediaway-ffi"],
  [/\bmediaway_(?:container|device|pipeline)_ffi\b/g, "mediaway_ffi"],
];
for (const f of sweepable()) {
  const orig = readFileSync(f, "utf8");
  let s = orig;
  for (const [re, rep] of nameMaps) s = s.replace(re, rep);
  if (s !== orig) {
    // verify: JSON files must stay parseable; TS files parseable by tree-sitter
    if (f.endsWith(".json")) {
      try {
        JSON.parse(s);
      } catch {
        throw new Error(`${f}: rewrite broke JSON`);
      }
    }
    plan.push({ what: `replace refs in ${f}`, detail: "ffi crate/DLL names" });
    edits.push({ path: f, old: orig, next: s });
  }
}

// ── step 5: draft parent Cargo.toml + src/lib.rs ────────────────────────────
function childToml(child) {
  const p = join(child.dir, "Cargo.toml");
  let txt;
  if (existsSync(p)) {
    txt = readFileSync(p, "utf8");
  } else {
    // already git rm'd by an earlier apply — read from HEAD
    txt = git("show", `HEAD:${toSlash(p)}`);
  }
  const sections = {};
  let cur = null;
  for (const line of txt.split("\n")) {
    const h = line.match(/^\[(.+)\]\s*$/);
    if (h) {
      cur = h[1];
      sections[cur] = sections[cur] ?? [];
      continue;
    }
    if (cur) sections[cur].push(line);
  }
  return sections;
}
{
  const childrenNames = spec.children.map((c) => c.name);
  const depLines = new Map(); // table -> ordered lines
  const features = new Map();
  const defaults = [];
  const crateTypes = new Set();
  for (const child of spec.children) {
    const t = childToml(child);
    const lib = (t["lib"] ?? []).join("\n");
    const ct = lib.match(/crate-type\s*=\s*\[([^\]]+)\]/);
    if (ct) ct[1].split(",").map((x) => x.trim().replace(/["']/g, "")).filter(Boolean).forEach((x) => crateTypes.add(x));
    for (const table of ["dependencies", "dev-dependencies", "target.'cfg(windows)'.dependencies", "target.'cfg(windows)'.dev-dependencies", "target.'cfg(target_os = \"linux\")'.dependencies"]) {
      const key = table;
      const bucket = depLines.get(key) ?? [];
      for (const line of t[key] ?? []) {
        const name = line.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
        if (!name) {
          // continuation line of a multi-line inline table — append to last entry
          if (bucket.length) bucket[bucket.length - 1] += "\n" + line;
          continue;
        }
        if (childrenNames.includes(name)) continue; // drop self/sibling refs
        if (!bucket.some((l) => l.startsWith(name + " = "))) bucket.push(line);
      }
      if (bucket.length) depLines.set(key, bucket);
    }
    for (const line of t.features ?? []) {
      const kv = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.*)$/);
      if (!kv) continue;
      if (kv[1] === "default") {
        kv[2].match(/\[([^\]]*)\]/)?.[1].split(",").map((x) => x.trim().replace(/["']/g, "")).filter(Boolean).forEach((x) => defaults.push(x));
        continue;
      }
      features.set(kv[1], line);
    }
    for (const line of t["lints"] ?? []) {
      // `workspace = true` — emit once
    }
  }
  const defaultLine = `default = [${[...new Set(defaults)].map((d) => `"${d}"`).join(", ")}]`;
  const parts = [
    `[package]`,
    `name = "${spec.crate}"`,
    spec.description ? `description = "${spec.description}"` : null,
    `version.workspace = true`,
    `publish = false # no header/ABI has shipped yet — see docs/roadmap.md`,
    `edition.workspace = true`,
    `rust-version.workspace = true`,
    `license.workspace = true`,
    `repository.workspace = true`,
    `authors.workspace = true`,
    ``,
    `[lib]`,
    `crate-type = [${[...crateTypes].map((x) => `"${x}"`).join(", ")}]`,
    ``,
  ].filter((x) => x !== null);
  const tables = [["dependencies", "dependencies"], ["dev-dependencies", "dev-dependencies"], ["target.'cfg(windows)'.dependencies", "target.'cfg(windows)'.dependencies"], ["target.'cfg(windows)'.dev-dependencies", "target.'cfg(windows)'.dev-dependencies"], ["target.'cfg(target_os = \"linux\")'.dependencies", "target.'cfg(target_os = \"linux\")'.dependencies"]];
  const draft = [...parts];
  for (const [key, header] of tables) {
    const lines = depLines.get(key) ?? [];
    if (lines.length === 0) continue;
    draft.push(`[${header}]`);
    draft.push(...lines);
    draft.push("");
  }
  draft.push(`[features]`);
  draft.push(defaultLine);
  for (const f of features.values()) draft.push(f);
  draft.push("");
  draft.push(`[lints]`);
  draft.push(`workspace = true`);
  draft.push("");
  plan.push({ what: `draft ${spec.dir}/Cargo.toml`, detail: "REVIEW: feature merge + dep pins" });
  edits.push({ path: join(spec.dir, "Cargo.toml"), old: "", next: draft.join("\n") });
  const libRs = `//! ${spec.description ?? `C ABI facade (merged per ADR-0021)`}
//!
//! Merged from: ${spec.children.map((c) => c.name).join(", ")} — see
//! ../../docs/adr/0021-workspace-consolidation.md.

#![allow(unsafe_code)] // FFI crate — see docs/conventions/code-style.md § unsafe

${spec.children.map((c) => `pub mod ${c.module};`).join("\n")}
`;
  plan.push({ what: `draft ${spec.dir}/src/lib.rs`, detail: "REVIEW: module wiring" });
  edits.push({ path: join(spec.dir, "src", "lib.rs"), old: "", next: libRs });
}

// ── output / execute ────────────────────────────────────────────────────────
if (!APPLY) {
  log(`DRY RUN — ${spec.crate} merge (${spec.children.map((c) => c.name).join(", ")})`);
  log("=".repeat(72));
  let i = 0;
  for (const p of plan) log(`${String(++i).padStart(3)}. ${p.what}${p.detail ? "  — " + p.detail : ""}`);
  log("=".repeat(72));
  for (const e of edits) {
    if (e.old === "") {
      log(`\n--- NEW ${e.path} ---\n${e.next}`);
    } else {
      log(`\n--- EDIT ${e.path} (${e.count} occurrence(s)) ---`);
      const a = e.old.split("\n"), b = e.next.split("\n");
      const N = Math.max(a.length, b.length);
      for (let k = 0; k < N; k++) {
        if (a[k] !== b[k]) log(`- ${a[k] ?? ""}\n+ ${b[k] ?? ""}`);
      }
    }
  }
  log(`\n[DRY RUN] ${edits.length} file edits + ${plan.length} operations planned. Re-run with --apply to execute.`);
  process.exit(0);
}

// apply
mkdirSync(join(spec.dir, "src"), { recursive: true });
for (const e of edits) {
  if (e.old === "") {
    // new file (draft scaffold)
    writeFileSync(e.path, e.next);
  } else {
    // in-place edit: replace the anchor/whole-file content once; a missing
    // anchor on re-run means a previous apply already did this edit — skip.
    const cur = existsSync(e.path) ? readFileSync(e.path, "utf8") : "";
    if (!cur.includes(e.old)) {
      log(`⚠ skip (already applied?): ${e.path}`);
      continue;
    }
    writeFileSync(e.path, cur.replace(e.old, e.next));
  }
}
log(`APPLIED: ${spec.crate} — ${edits.length} file writes + moves executed.`);
log("Review before building:");
log("  cargo check -p " + spec.crate);
log("  cargo fmt --all && cargo clippy -p " + spec.crate);
log("Manual follow-ups (per spec): bindings scripts, ci.yml/release.yml -p flags, DLL names.");
