#!/usr/bin/env bun
/**
 * merge-crate.ts — merge child crates into a parent crate as modules (ADR-0021).
 *
 * Mechanical, reviewable, history-preserving (git mv), dry-run by default:
 *
 *   bun tools/scripts/merge-crate.ts <spec.json>          # plan only
 *   bun tools/scripts/merge-crate.ts <spec.json> --apply  # execute
 *
 * Spec:
 *   {
 *     "crate": "mediaway-device",
 *     "dir": "crates/mediaway-device",   // parent dir (may already exist)
 *     "description": "…",                // used only when scaffolding a NEW parent
 *     "rootUnsafe": "allow",             // replace parent lib.rs #![forbid(unsafe_code)]
 *                                        // with #![allow(unsafe_code)] (platform backends)
 *     "children": [
 *       { "name": "mediaway-device-camera", "dir": "crates/mediaway-device-camera", "module": "camera" },
 *       …
 *     ],
 *     "renames": [                       // merged-src sibling/parent path rewrites,
 *       ["mediaway_device_camera", "crate::camera::"],   // applied as `X::` -> path
 *       ["mediaway_device", "crate::"]
 *     ],
 *     "depSweep": [                      // dependent .rs rewrites (outside merged dirs)
 *       ["mediaway_device_camera", "mediaway_device::camera::"]
 *     ]
 *   }
 *
 * Steps (planned first, executed with --apply):
 *   1. git mv each child src -> <dir>/src/<module> (lib.rs -> mod.rs); tests/benches/
 *      adr/docs/README.md -> <dir>/{tests,benches,adr,docs}/<module>/; include/* merged.
 *      git rm child Cargo.toml.
 *   2. Root workspace Cargo.toml: drop children from members + [workspace.dependencies]
 *      (parent ADD queued before removals), add parent if new.
 *   3. Merged-src Rust rewrites (tree-sitter-rust targeted where noted):
 *        - `crate::` -> `crate::<module>::` (self root refs; skips existing module refs)
 *        - `renames` entries: `<old>::` -> `<path>`
 *        - doc-link depth (`../../../docs/` -> `../../../../docs/`)
 *      Integration tests: `<child>_::` -> `<parent>_::<module>::`
 *   4. Dependent Rust sweep (tracked .rs outside <dir>/): `depSweep` entries.
 *   5. Dependent Cargo.tomls: drop child dep entries (add parent entry only when the
 *      parent dep is not already present). Feature tables referencing removed deps
 *      (`dep:mediaway-device-camera`) are NOT rewritten — flag for manual fix.
 *   6. Parent Cargo.toml + src/lib.rs:
 *        - parent absent: scaffold draft (union of child deps/features)
 *        - parent present: merge child deps into existing tables (no duplicates, no
 *          self/sibling refs), union [features], append `pub mod <module>;` to lib.rs,
 *          apply rootUnsafe
 *   7. Non-Rust name replacements (tracked bindings/tools/docs, word-boundary,
 *      tree-sitter-json/typescript verified where parseable): ffi crate/DLL names.
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
const PARENT_EXISTS = existsSync(join(spec.dir, "Cargo.toml"));

const log = (s = "") => console.log(s);
const plan = [];
const edits = []; // { path, old, next, count? }
const movedOrig = new Map(); // moved target -> original path (dry-run reads originals)

function git(...cmd) {
  return execFileSync("git", cmd, { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}
const toSlash = (p) => p.replace(/\\/g, "/");
function isTracked(p) {
  try { git("ls-files", "--error-unmatch", "--", p); return true; } catch { return false; }
}
function listFiles(dir) {
  return git("ls-files", dir).split("\n").filter(Boolean).map(toSlash);
}
function queueEdit(path, old, next) {
  const cur = readFileSync(path, "utf8");
  if (!cur.includes(old)) throw new Error(`${path}: anchor not found: ${JSON.stringify(old.slice(0, 60))}`);
  edits.push({ path, old, next, count: cur.split(old).length - 1 });
}

// ── tree-sitter ─────────────────────────────────────────────────────────────
const parser = new Parser(); parser.setLanguage(RustLang);
const jsonParser = new Parser(); jsonParser.setLanguage(JsonLang);
const tsParser = new Parser(); tsParser.setLanguage(TsLang);

// ── step 1: moves ───────────────────────────────────────────────────────────
const movedFiles = [];
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
    plan.push({ what: `git mv ${toSlash(srcDir)}/ -> ${toSlash(dest)}/`, detail: `${listFiles(srcDir).length} files` });
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
  const incDir = join(child.dir, "include");
  if (existsSync(incDir)) {
    for (const f of listFiles(incDir)) {
      const target = join(spec.dir, "include", relative(incDir, f));
      plan.push({ what: `git mv ${f} -> ${toSlash(target)}`, detail: "" });
      movedFiles.push(toSlash(target));
      movedOrig.set(toSlash(target), toSlash(f));
      if (APPLY) {
        mkdirSync(dirname(target), { recursive: true });
        execFileSync("git", ["mv", "--", f, target], { cwd: ROOT, stdio: "ignore" });
      }
    }
  }
  // module root file: src/<module>/lib.rs -> mod.rs
  {
    const lib = join(spec.dir, "src", child.module, "lib.rs");
    if (existsSync(lib) || (APPLY && existsSync(join(spec.dir, "src", child.module)))) {
      plan.push({ what: `git mv ${toSlash(join(spec.dir, "src", child.module, "lib.rs"))} -> mod.rs`, detail: "dir-module root" });
      if (APPLY && existsSync(lib)) execFileSync("git", ["mv", "--", toSlash(lib), toSlash(join(spec.dir, "src", child.module, "mod.rs"))], { cwd: ROOT, stdio: "ignore" });
    }
  }
  const readme = join(child.dir, "README.md");
  if (existsSync(readme) && isTracked(readme)) {
    const target = join(spec.dir, "docs", child.module, "README.md");
    plan.push({ what: `git mv ${readme} -> ${toSlash(target)}`, detail: "" });
    movedFiles.push(toSlash(target));
    movedOrig.set(toSlash(target), toSlash(readme));
    if (APPLY) {
      mkdirSync(dirname(target), { recursive: true });
      execFileSync("git", ["mv", "--", readme, target], { cwd: ROOT, stdio: "ignore" });
    }
  }
  plan.push({ what: `git rm ${child.dir}/Cargo.toml`, detail: "deps merged into parent" });
  if (APPLY && existsSync(join(child.dir, "Cargo.toml")))
    execFileSync("git", ["rm", "--", join(child.dir, "Cargo.toml")], { cwd: ROOT, stdio: "ignore" });
}

// ── step 2: root workspace manifest ─────────────────────────────────────────
{
  const wc = readFileSync("Cargo.toml", "utf8");
  const childDirs = spec.children.map((c) => toSlash(c.dir));
  const membersRe = /^(\s*)"(crates\/[^"]+)",\s*$/gm;
  const memberLines = [];
  let m;
  while ((m = membersRe.exec(wc)) !== null) memberLines.push(m);
  const present = memberLines.filter((x) => childDirs.includes(x[2]));
  const want = toSlash(spec.dir);
  // parent ADD first (its anchor line is deleted by the removals below)
  if (!memberLines.some((x) => x[2] === want)) {
    const anchor = present[0] ?? memberLines.find((x) => x[2] < want);
    if (anchor) {
      plan.push({ what: `Cargo.toml members: add "${want}"`, detail: "at first removed position" });
      queueEdit("Cargo.toml", anchor[0], `    "${want}",\n${anchor[0]}`);
    } else {
      plan.push({ what: "Cargo.toml members: add MANUALLY (no anchor)", detail: want });
    }
  }
  for (const p of present) {
    plan.push({ what: `Cargo.toml members: remove "${p[2]}"`, detail: "" });
    queueEdit("Cargo.toml", p[0], "");
  }
  const parentDep = `${spec.crate} = { path = "${want}", version = "0.1.0" }`;
  if (!wc.includes(parentDep)) {
    const firstChild = spec.children.find((c) => wc.includes(`${c.name} = { path = "`));
    const anchor = firstChild && wc.match(new RegExp(`^${firstChild.name} = \\{ path = "[^"]+", version = "[^"]+" \\}`, "m"));
    plan.push({ what: `Cargo.toml [workspace.dependencies]: add ${parentDep}`, detail: "" });
    if (anchor) queueEdit("Cargo.toml", anchor[0], `${parentDep}\n${anchor[0]}`);
    else plan.push({ what: "Cargo.toml: add parent dep MANUALLY (no anchor)", detail: parentDep });
  }
  for (const child of spec.children) {
    const re = new RegExp(`^(${child.name} = \\{ path = "[^"]+", version = "[^"]+" \\}\\s*)$`, "m");
    const hit = wc.match(re);
    if (!hit) { log(`⚠ [workspace.dependencies]: no entry for ${child.name} — check manually`); continue; }
    plan.push({ what: `Cargo.toml [workspace.dependencies]: remove ${child.name}`, detail: "" });
    queueEdit("Cargo.toml", hit[1], "");
  }
}

// ── step 3: merged-src Rust rewrites ────────────────────────────────────────
const parentUnderscore = spec.crate.replace(/-/g, "_");
function mergedSrcFiles(child) {
  return git("ls-files", toSlash(join(spec.dir, "src", child.module)))
    .split("\n").filter(Boolean).map(toSlash).filter((f) => f.endsWith(".rs"))
    .concat(movedFiles.filter((f) => f.startsWith(`${toSlash(spec.dir)}/src/${child.module}/`) && f.endsWith(".rs")))
    .filter((v, i, a) => a.indexOf(v) === i);
}
for (const child of spec.children) {
  for (const f of mergedSrcFiles(child)) {
    const srcPath = APPLY ? f : (movedOrig.get(f) ?? f);
    if (!existsSync(srcPath)) continue;
    const orig = readFileSync(srcPath, "utf8");
    let s = orig;
    // (a) self root refs gain the module prefix (skip existing module/common refs)
    const skip = `(?:${child.module}::|common::)`;
    s = s.replace(new RegExp(`crate::(?!${skip})`, "g"), `crate::${child.module}::`);
    // (b) sibling/parent renames (after (a): their `crate::` prefixes are new)
    for (const [old, path] of spec.renames ?? []) {
      s = s.split(`${old}::`).join(path);
      s = s.split(`use ${old} as `).join(`use ${path.replace(/::$/, "")} as `);
    }
    // (c) doc-link depth
    s = s.split("../../../docs/").join("../../../../docs/");
    if (s !== orig) {
      plan.push({ what: `rewrite ${f}`, detail: `module src (rust)` });
      edits.push({ path: f, old: orig, next: s, count: 1 });
    }
  }
  for (const f of mergedSrcFiles(child).filter((f) => f.startsWith(`${toSlash(spec.dir)}/tests/`))) {
    const srcPath = APPLY ? f : (movedOrig.get(f) ?? f);
    if (!existsSync(srcPath)) continue;
    const orig = readFileSync(srcPath, "utf8");
    const childU = child.name.replace(/-/g, "_");
    const s = orig.split(`${childU}::`).join(`${parentUnderscore}::${child.module}::`);
    if (s !== orig) {
      plan.push({ what: `rewrite ${f}`, detail: `test crate ref` });
      edits.push({ path: f, old: orig, next: s, count: 1 });
    }
  }
}

// ── step 4: dependent Rust sweep (outside merged dirs) ──────────────────────
if (spec.depSweep?.length) {
  const depRust = git("ls-files", "crates", "tools", "examples")
    .split("\n").filter(Boolean).map(toSlash)
    .filter((f) => f.endsWith(".rs") && !f.startsWith(`${toSlash(spec.dir)}/`));
  for (const f of depRust) {
    const orig = readFileSync(f, "utf8");
    let s = orig;
    for (const [old, path] of spec.depSweep) {
      s = s.split(`${old}::`).join(path);
      s = s.split(`use ${old} as `).join(`use ${path.replace(/::$/, "")} as `);
    }
    if (s !== orig) {
      plan.push({ what: `rewrite ${f}`, detail: `dependent rust refs` });
      edits.push({ path: f, old: orig, next: s, count: 1 });
    }
  }
}

// ── step 5: dependent Cargo.tomls ───────────────────────────────────────────
{
  const childNames = spec.children.map((c) => c.name);
  const depTomls = git("ls-files", "crates", "tools", "examples")
    .split("\n").filter(Boolean).map(toSlash)
    .filter((f) => f.endsWith("Cargo.toml") && !childNames.some((n) => f.includes(`/${n}/`)) && !f.startsWith(`${toSlash(spec.dir)}/`));
  for (const f of depTomls) {
    const orig = readFileSync(f, "utf8");
    let s = orig;
    let touched = false;
    // line-anchored entries, skipping multi-line inline tables until braces balance
    const lines = s.split("\n");
    const out = [];
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const name = line.match(/^([A-Za-z0-9_-]+)\s*=\s*\{/)?.[1];
      if (name && childNames.includes(name)) {
        // skip until the entry closes (balanced braces, best-effort: `] }` or `}`)
        let depth = 0;
        for (let j = i; j < lines.length; j++) {
          depth += (lines[j].match(/\{/g) ?? []).length - (lines[j].match(/\}/g) ?? []).length;
          if (depth <= 0) { i = j; break; }
        }
        touched = true;
        continue;
      }
      out.push(line);
    }
    if (touched && out.join("\n") !== s) {
      // ensure parent dep exists when the file references the parent path already
      const needsParent = orig.includes("mediaway-") && childNames.some((n) => orig.includes(n));
      const hasParent = out.some((l) => l.startsWith(`${spec.crate} = `) || l.includes(`"${spec.crate}"`));
      if (needsParent && !hasParent) {
        // append to [dependencies] if present
        const di = out.findIndex((l) => l.trim() === "[dependencies]");
        if (di >= 0) {
          out.splice(di + 1, 0, `${spec.crate} = { workspace = true }`);
        } else {
          plan.push({ what: `${f}: parent dep add MANUAL (no [dependencies])`, detail: spec.crate });
        }
      }
      const next = out.join("\n");
      plan.push({ what: `rewrite ${f}`, detail: `dependent cargo deps` });
      edits.push({ path: f, old: orig, next, count: 1 });
    }
  }
}

// ── step 6: parent Cargo.toml + lib.rs ──────────────────────────────────────
function childToml(child) {
  const p = join(child.dir, "Cargo.toml");
  let txt;
  if (existsSync(p)) txt = readFileSync(p, "utf8");
  else txt = git("show", `HEAD:${toSlash(p)}`);
  const sections = {};
  let cur = null;
  for (const line of txt.split("\n")) {
    const h = line.match(/^\[(.+)\]\s*$/);
    if (h) { cur = h[1]; sections[cur] = sections[cur] ?? []; continue; }
    if (cur) sections[cur].push(line);
  }
  return sections;
}
function mergeDepLines(depLines, table, t, childNames) {
  const bucket = depLines.get(table) ?? [];
  for (const line of t[table] ?? []) {
    const name = line.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
    if (!name) { if (bucket.length) bucket[bucket.length - 1] += "\n" + line; continue; }
    if (childNames.includes(name) || name === spec.crate) continue;
    if (!bucket.some((l) => l.startsWith(name + " = "))) bucket.push(line);
  }
  if (bucket.length) depLines.set(table, bucket);
}
{
  const childNames = spec.children.map((c) => c.name);
  const childTomls = spec.children.map(childToml);
  // standard tables + every target-cfg table children declare (windows/linux/wasm32/…)
  const depTables = [
    "dependencies",
    "dev-dependencies",
    ...new Set(childTomls.flatMap((t) => Object.keys(t).filter((k) => k.startsWith("target.")))),
  ];
  const features = new Map();
  const defaults = [];
  const crateTypes = new Set();

  if (PARENT_EXISTS) {
    // merge into existing parent manifest
    const p = join(spec.dir, "Cargo.toml");
    const orig = readFileSync(p, "utf8");
    const sections = {};
    let cur = null;
    for (const line of orig.split("\n")) {
      const h = line.match(/^\[(.+)\]\s*$/);
      if (h) { cur = h[1]; sections[cur] = sections[cur] ?? []; continue; }
      if (cur) sections[cur].push(line);
    }
    const depLines = new Map();
    for (const t of depTables) depLines.set(t, [...(sections[t] ?? [])]);
    for (const t of childTomls) for (const table of depTables) mergeDepLines(depLines, table, t, childNames);
    for (const line of sections.features ?? []) {
      const kv = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.*)$/);
      if (!kv) continue;
      if (kv[1] === "default") { kv[2].match(/\[([^\]]*)\]/)?.[1].split(",").map((x) => x.trim().replace(/["']/g, "")).filter(Boolean).forEach((x) => defaults.push(x)); continue; }
      features.set(kv[1], line);
    }
    for (const t of childTomls) for (const line of t.features ?? []) {
      const kv = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.*)$/);
      if (!kv) continue;
      if (kv[1] === "default") { kv[2].match(/\[([^\]]*)\]/)?.[1].split(",").map((x) => x.trim().replace(/["']/g, "")).filter(Boolean).forEach((x) => defaults.push(x)); continue; }
      if (!features.has(kv[1])) features.set(kv[1], line);
    }
    // rebuild the manifest preserving non-dep tables in order; append child-only
    // target tables (e.g. wasm32 deps the parent never had)
    const order = [...Object.keys(sections)];
    for (const t of depTables) if (!order.includes(t) && (depLines.get(t) ?? []).length) order.push(t);
    const parts = [];
    for (const t of order) {
      if (t === "features") continue;
      if (depTables.includes(t)) {
        const lines = depLines.get(t) ?? [];
        if (lines.length === 0) continue;
        parts.push(`[${t}]`, ...lines, "");
      } else {
        parts.push(`[${t}]`, ...(sections[t] ?? []), "");
      }
    }
    parts.push(`[features]`);
    parts.push(`default = [${[...new Set(defaults)].map((d) => `"${d}"`).join(", ")}]`);
    for (const f of features.values()) parts.push(f);
    parts.push("");
    const next = parts.join("\n");
    if (next !== orig) {
      plan.push({ what: `merge deps into ${toSlash(p)}`, detail: "REVIEW: feature merge" });
      edits.push({ path: p, old: orig, next, count: 1 });
    }
    // lib.rs: append module decls + rootUnsafe
    const lr = join(spec.dir, "src", "lib.rs");
    if (existsSync(lr)) {
      const lOrig = readFileSync(lr, "utf8");
      let l = lOrig;
      if (spec.rootUnsafe) {
        const attr = `#![${spec.rootUnsafe}(unsafe_code)]`;
        const reForbid = /#!\[forbid\(unsafe_code\)\]/;
        const reDeny = /#!\[deny\(unsafe_code\)\]/;
        const reAllow = /#!\[allow\(unsafe_code\)\]/;
        if (reForbid.test(l) || reDeny.test(l) || reAllow.test(l)) {
          l = l.replace(reForbid, attr).replace(reDeny, attr).replace(reAllow, attr);
          plan.push({ what: `lib.rs: unsafe attr -> ${spec.rootUnsafe}`, detail: "platform backends merged in" });
        }
      }
      const mods = spec.children.map((c) => `pub mod ${c.module};`).join("\n");
      if (!l.includes(`pub mod ${spec.children[0].module};`)) {
        l = l.replace(/\s*$/, "\n\n// ── merged platform/domain modules (ADR-0021) ──\n" + mods + "\n");
        plan.push({ what: `lib.rs: append ${spec.children.length} module decls`, detail: "REVIEW: cfg gating" });
      }
      if (l !== lOrig) edits.push({ path: lr, old: lOrig, next: l, count: 1 });
    }
  } else {
    // scaffold a new parent (ffi-style): union draft
    for (const t of childTomls) {
      const lib = (t["lib"] ?? []).join("\n");
      const ct = lib.match(/crate-type\s*=\s*\[([^\]]+)\]/);
      if (ct) ct[1].split(",").map((x) => x.trim().replace(/["']/g, "")).filter(Boolean).forEach((x) => crateTypes.add(x));
    }
    const depLines = new Map();
    for (const table of depTables) depLines.set(table, []);
    for (const t of childTomls) for (const table of depTables) mergeDepLines(depLines, table, t, childNames);
    for (const t of childTomls) for (const line of t.features ?? []) {
      const kv = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.*)$/);
      if (!kv) continue;
      if (kv[1] === "default") { kv[2].match(/\[([^\]]*)\]/)?.[1].split(",").map((x) => x.trim().replace(/["']/g, "")).filter(Boolean).forEach((x) => defaults.push(x)); continue; }
      features.set(kv[1], line);
    }
    const parts = [
      `[package]`, `name = "${spec.crate}"`,
      spec.description ? `description = "${spec.description}"` : null,
      `version.workspace = true`, `publish = false # early development — see docs/roadmap.md`,
      `edition.workspace = true`, `rust-version.workspace = true`, `license.workspace = true`,
      `repository.workspace = true`, `authors.workspace = true`, ``,
      `[lib]`, `crate-type = [${[...crateTypes].map((x) => `"${x}"`).join(", ")}]`, ``,
    ].filter((x) => x !== null);
    for (const table of depTables) {
      const lines = depLines.get(table) ?? [];
      if (lines.length === 0) continue;
      parts.push(`[${table}]`, ...lines, "");
    }
    parts.push(`[features]`, `default = [${[...new Set(defaults)].map((d) => `"${d}"`).join(", ")}]`);
    for (const f of features.values()) parts.push(f);
    parts.push("", `[lints]`, `workspace = true`, "");
    plan.push({ what: `draft ${spec.dir}/Cargo.toml`, detail: "REVIEW: feature merge + dep pins" });
    edits.push({ path: join(spec.dir, "Cargo.toml"), old: "", next: parts.join("\n") });
    const libRs = `//! ${spec.description ?? spec.crate}\n//!\n//! Merged from: ${spec.children.map((c) => c.name).join(", ")} — ADR-0021.\n\n#![allow(unsafe_code)] // FFI/platform crate — SAFETY comments per file\n\n${spec.children.map((c) => `pub mod ${c.module};`).join("\n")}\n`;
    plan.push({ what: `draft ${spec.dir}/src/lib.rs`, detail: "REVIEW: module wiring" });
    edits.push({ path: join(spec.dir, "src", "lib.rs"), old: "", next: libRs });
  }
}

// ── step 7: non-Rust name replacements ──────────────────────────────────────
const SKIP_DIRS = ["node_modules", "target", "dist", "build", "runtime", "pkg", ".git", "obj", "bin", "specs"];
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
    if (f.endsWith(".json")) { try { JSON.parse(s); } catch { throw new Error(`${f}: rewrite broke JSON`); } }
    plan.push({ what: `replace refs in ${f}`, detail: "ffi crate/DLL names" });
    edits.push({ path: f, old: orig, next: s, count: 1 });
  }
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
      log(`\n--- EDIT ${e.path} (${e.count ?? 1} occurrence(s)) ---`);
      const a = e.old.split("\n"), b = e.next.split("\n");
      const N = Math.max(a.length, b.length);
      for (let k = 0; k < N; k++) {
        if (a[k] !== b[k]) log(`- ${a[k] ?? ""}\n+ ${b[k] ?? ""}`);
      }
    }
  }
  log(`\n[DRY RUN] ${edits.length} file edits planned. Re-run with --apply to execute.`);
  process.exit(0);
}

mkdirSync(join(spec.dir, "src"), { recursive: true });
let skipped = 0;
for (const e of edits) {
  if (e.old === "") {
    writeFileSync(e.path, e.next);
  } else {
    const cur = existsSync(e.path) ? readFileSync(e.path, "utf8") : "";
    if (!cur.includes(e.old)) { skipped++; log(`⚠ skip (already applied?): ${e.path}`); continue; }
    writeFileSync(e.path, cur.replace(e.old, e.next));
  }
}
log(`APPLIED: ${spec.crate} — ${edits.length} edits (${skipped} skipped).`);
log("Review before building:");
log(`  cargo check -p ${spec.crate}`);
log(`  cargo test -p ${spec.crate}`);
log("Manual follow-ups: feature tables referencing removed deps (dep:…), bindings scripts.");
