#!/usr/bin/env bun
/**
 * Incremental coverage check — changed Rust files since a baseline → llvm-cov run →
 * report (test counts, line coverage, per-changed-file delta vs baseline).
 *
 * Daily/dev-loop only. Do **not** wire into pre-push / PR CI gates.
 *
 * Baseline resolution order:
 *   1. `--since <ref>`           — explicit
 *   2. baseline file (default `local/.cache/coverage/baseline.json`) — its `commit`
 *   3. last commit ≥ 24h old      — `git rev-list -1 --before="24 hours ago" HEAD`
 *   4. HEAD                       — first run
 *
 * Usage:
 *   bun run coverage-check.ts
 *   bun run coverage-check.ts --since main
 *   bun run coverage-check.ts --baseline <file> --no-store
 *   bun run coverage-check.ts --output <report.md>
 *
 * Requires: cargo-llvm-cov, cargo-nextest (cargo install cargo-llvm-cov cargo-nextest)
 * Policy: docs/conventions/testing.md § Incremental (dev)
 */

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

interface Args {
  since: string | null;
  baseline: string;
  store: boolean;
  output: string | null;
}

function usage(): never {
  console.error(`Usage: bun run coverage-check.ts [--since <ref>] [--baseline <file>] [--no-store] [--output <file>]

Baseline order: --since > baseline file (local/.cache/coverage/baseline.json) > last commit ≥24h old > HEAD.
Dev-loop + daily CI only — not a merge gate.`);
  process.exit(2);
}

function parseArgs(argv: string[]): Args {
  const args: Args = { since: null, baseline: join(import.meta.dir, "..", "..", "local", ".cache", "coverage", "baseline.json"), store: true, output: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--since") {
      const v = argv[++i];
      if (!v) usage();
      args.since = v;
    } else if (a === "--baseline") {
      const v = argv[++i];
      if (!v) usage();
      args.baseline = v;
    } else if (a === "--no-store") {
      args.store = false;
    } else if (a === "--output") {
      const v = argv[++i];
      if (!v) usage();
      args.output = v;
    } else if (a === "-h" || a === "--help") {
      usage();
    } else {
      usage();
    }
  }
  return args;
}

function runCapture(cmd: string, args: string[]): { status: number; stdout: string; stderr: string } {
  const r = spawnSync(cmd, args, { encoding: "utf8", shell: true, maxBuffer: 256 * 1024 * 1024 });
  if (r.error) {
    return { status: 1, stdout: r.stdout ?? "", stderr: String(r.error) };
  }
  return { status: r.status ?? 1, stdout: r.stdout ?? "", stderr: r.stderr ?? "" };
}

const root = join(import.meta.dir, "..", "..");

interface Baseline {
  commit: string;
  date: string;
  tests: { passed: number; failed: number; skipped: number };
  total: { lf: number; lh: number };
  files: Record<string, { lf: number; lh: number }>;
}

function readBaseline(path: string): Baseline | null {
  if (!existsSync(path)) return null;
  try {
    return JSON.parse(readFileSync(path, "utf8")) as Baseline;
  } catch {
    return null;
  }
}

function git(args: string[]): { status: number; out: string } {
  const r = runCapture("git", args);
  return { status: r.status, out: r.stdout.trim() };
}

// ── lcov parsing ────────────────────────────────────────────────────────────────
// `--output-format lcov` emits: SF:<path> / LF:<lines found> / LH:<lines hit> blocks.
function parseLcov(text: string): { total: { lf: number; lh: number }; files: Record<string, { lf: number; lh: number }> } {
  const files: Record<string, { lf: number; lh: number }> = {};
  const re = /SF:(.+)\n(?:[^]*?LF:(\d+)\nLH:(\d+))/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    const path = m[1].replaceAll("\\", "/");
    const lf = Number(m[2]);
    const lh = Number(m[3]);
    if (Number.isFinite(lf) && Number.isFinite(lh) && path.includes("/")) files[path] = { lf, lh };
  }
  let tLf = 0, tLh = 0;
  for (const f of Object.values(files)) { tLf += f.lf; tLh += f.lh; }
  return { total: { lf: tLf, lh: tLh }, files };
}

function pct(lh: number, lf: number): string {
  return lf === 0 ? "—" : `${((100 * lh) / lf).toFixed(1)}%`;
}

// ── main ────────────────────────────────────────────────────────────────────────
function main(): void {
  const args = parseArgs(process.argv.slice(2));

  for (const tool of ["cargo-llvm-cov", "cargo-nextest"]) {
    if (!existsSync(join(import.meta.dir, "node_modules"))) {
      // no check needed — these are cargo subcommands, verified below
    }
    const r = runCapture("cargo", [tool === "cargo-llvm-cov" ? "llvm-cov" : "nextest", "--version"]);
    if (r.status !== 0) {
      console.error(`cargo ${tool.replace("cargo-", "")} not available — install: cargo install ${tool}`);
      process.exit(1);
    }
  }

  const prev = readBaseline(args.baseline);
  const head = git(["rev-parse", "--short", "HEAD"]);
  let since = args.since;
  if (!since && prev?.commit) since = prev.commit;
  if (!since) {
    const day = git(["rev-list", "-1", "--before=24 hours ago", "HEAD"]);
    since = day.status === 0 && day.out ? day.out : head.out;
  }
  const sinceShort = since.length > 12 ? since.slice(0, 12) : since;

  // Changed Rust files (committed after baseline + uncommitted working tree).
  const diff = git(["diff", "--name-only", since]);
  const changed = diff.out.split("\n").filter((p) => p.endsWith(".rs") && (p.startsWith("crates/") || p.startsWith("tools/") || p.startsWith("examples/")));
  const changedSet = new Set(changed.map((p) => p.replaceAll("\\", "/")));

  // Run instrumented tests (no report yet), then export lcov.
  const run = runCapture("cargo", ["llvm-cov", "nextest", "--workspace", "--no-report"]);
  if (run.status !== 0) {
    console.error("coverage run failed:\n" + run.stderr.slice(-2000));
    process.exit(run.status || 1);
  }
  const reportRun = runCapture("cargo", ["llvm-cov", "report", "--lcov"]);
  if (reportRun.status !== 0 || !reportRun.stdout.includes("SF:")) {
    console.error("llvm-cov report failed:\n" + reportRun.stderr.slice(-2000));
    process.exit(reportRun.status || 1);
  }
  const cov = parseLcov(reportRun.stdout);

  // Test counts from the runner summary. Formats:
  //   "Summary [   8.039s] 851 tests run: 851 passed, 0 skipped"   (nextest)
  //   "Summary: N tests passed, M tests failed, K tests skipped"    (cargo test)
  // Groups: 1 = nextest total, 2 = nextest passed, 3 = cargo-test passed,
  //         4 = failed, 5 = skipped.
  const sumRe =
    /(?:Summary\s*\[[^\]]*\]\s*(\d+)\s+tests?\s+run:\s*(\d+)\s+passed|Summary:\s*(\d+)\s+tests?\s+passed)(?:,\s*(\d+)\s+failed)?(?:,\s*(\d+)\s+skipped)?/i;
  const sumM = sumRe.exec(run.stderr) ?? sumRe.exec(run.stdout);
  const tests = sumM
    ? { passed: Number(sumM[2] ?? sumM[3] ?? 0), failed: Number(sumM[4] ?? 0), skipped: Number(sumM[5] ?? 0) }
    : { passed: 0, failed: 0, skipped: 0 };

  // Report.
  const lines: string[] = [];
  lines.push(`## Coverage check — ${new Date().toISOString().slice(0, 10)}`);
  lines.push("");
  lines.push(`baseline: \`${sinceShort}\` → head: \`${head.out}\``);
  lines.push(`changed Rust files: ${changed.length}`);
  lines.push(`tests: **${tests.passed} passed**, ${tests.failed} failed, ${tests.skipped} skipped`);
  lines.push(`total line coverage: **${pct(cov.total.lh, cov.total.lf)}** (${cov.total.lh}/${cov.total.lf})`);
  if (prev) {
    const d = cov.total.lf > 0 ? (100 * cov.total.lh) / cov.total.lf - (100 * prev.total.lh) / prev.total.lf : 0;
    lines.push(`delta vs baseline (\`${prev.commit}\`): ${d >= 0 ? "+" : ""}${d.toFixed(1)}pp`);
  }
  lines.push("");
  if (changed.length > 0) {
    lines.push("| File | Line cov | vs baseline |");
    lines.push("| ---- | -------- | ----------- |");
    for (const f of changed) {
      const cur = cov.files[f];
      const old = prev?.files[f];
      const cell = cur ? pct(cur.lh, cur.lf) : "not instrumented";
      const delta = cur && old && old.lf > 0
        ? `${((100 * cur.lh) / cur.lf - (100 * old.lh) / old.lf).toFixed(1)}pp`
        : (cur ? "new" : "—");
      lines.push(`| \`${f}\` | ${cell} | ${delta} |`);
    }
  } else {
    lines.push("No changed Rust files since baseline.");
  }

  const report = lines.join("\n") + "\n";
  if (args.output) {
    writeFileSync(args.output, report, "utf8");
    console.log(`report written to ${args.output}`);
  } else {
    console.log(report);
  }

  if (args.store) {
    const b: Baseline = {
      commit: head.out,
      date: new Date().toISOString(),
      tests,
      total: cov.total,
      files: cov.files,
    };
    mkdirSync(join(import.meta.dir, "..", "..", "local", ".cache", "coverage"), { recursive: true });
    writeFileSync(args.baseline, JSON.stringify(b, null, 2), "utf8");
    console.log(`baseline stored: ${args.baseline}`);
  }
}

main();
