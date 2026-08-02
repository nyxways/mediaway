#!/usr/bin/env bun
/**
 * Incremental tests — cargo-impact blast radius → cargo nextest filter.
 *
 * Dev-loop only. Do **not** wire into pre-push / PR CI gates
 * (false negatives; full suite remains the merge gate).
 *
 * Usage:
 *   bun run incremental-test.ts
 *   bun run incremental-test.ts --since main
 *   bun run incremental-test.ts --since HEAD --confidence-min 0.3
 *   bun run incremental-test.ts -- --no-fail-fast
 *
 * Requires: cargo-impact, cargo-nextest
 * Policy: docs/conventions/testing.md § Incremental (dev)
 */

import { spawnSync } from "node:child_process";

function usage(): never {
  console.error(`Usage: bun run incremental-test.ts [--since <ref>] [--confidence-min <n>] [-- <nextest args>]

Defaults: --since HEAD --confidence-min 0.5
Dev-loop only — not a CI / pre-push gate.`);
  process.exit(2);
}

function parseArgs(argv: string[]): {
  since: string;
  confidenceMin: string;
  nextestArgs: string[];
} {
  let since = "HEAD";
  let confidenceMin = "0.5";
  const nextestArgs: string[] = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--since") {
      const v = argv[++i];
      if (!v) usage();
      since = v;
    } else if (a === "--confidence-min") {
      const v = argv[++i];
      if (!v) usage();
      confidenceMin = v;
    } else if (a === "--") {
      nextestArgs.push(...argv.slice(i + 1));
      break;
    } else if (a === "-h" || a === "--help") {
      usage();
    } else {
      nextestArgs.push(a);
    }
  }
  return { since, confidenceMin, nextestArgs };
}

function haveCmd(cmd: string): boolean {
  const r = spawnSync(cmd, ["--version"], { encoding: "utf8", shell: true });
  return r.status === 0;
}

function runCapture(cmd: string, args: string[]): { status: number; stdout: string; stderr: string } {
  const r = spawnSync(cmd, args, { encoding: "utf8", shell: true });
  return {
    status: r.status ?? 1,
    stdout: r.stdout ?? "",
    stderr: r.stderr ?? "",
  };
}

function runInherit(cmd: string, args: string[]): number {
  const r = spawnSync(cmd, args, { encoding: "utf8", shell: true, stdio: "inherit" });
  return r.status ?? 1;
}

function main(): void {
  const { since, confidenceMin, nextestArgs } = parseArgs(process.argv.slice(2));

  if (!haveCmd("cargo-impact") && !haveCmd("cargo")) {
    console.error("cargo not found on PATH");
    process.exit(1);
  }

  // cargo-impact installs as `cargo-impact` and is also invoked as `cargo impact`
  const impactCheck = spawnSync("cargo", ["impact", "--help"], {
    encoding: "utf8",
    shell: true,
  });
  if (impactCheck.status !== 0) {
    console.error("cargo impact not available — install: cargo install cargo-impact");
    console.error("See docs/conventions/testing.md § Incremental (dev)");
    process.exit(1);
  }

  const nextestCheck = spawnSync("cargo", ["nextest", "--version"], {
    encoding: "utf8",
    shell: true,
  });
  if (nextestCheck.status !== 0) {
    console.error("cargo nextest not available — install: cargo install cargo-nextest");
    process.exit(1);
  }

  console.error(
    `[1/2] cargo impact --since ${since} --confidence-min ${confidenceMin} --test`,
  );
  const t0 = performance.now();
  const impact = runCapture("cargo", [
    "impact",
    "--since",
    since,
    "--confidence-min",
    confidenceMin,
    "--test",
  ]);
  const ms = Math.round(performance.now() - t0);
  console.error(`    graph analysis ${ms} ms`);

  if (impact.status !== 0) {
    console.error(impact.stderr || impact.stdout);
    process.exit(impact.status);
  }

  const filter = impact.stdout.trim();
  if (!filter) {
    console.error(`    no tests impacted by changes since ${since}`);
    process.exit(0);
  }

  console.error(`    filter: ${filter}`);
  console.error("");
  console.error(`[2/2] cargo nextest run --workspace -E '<filter>' ${nextestArgs.join(" ")}`);
  const code = runInherit("cargo", [
    "nextest",
    "run",
    "--workspace",
    "-E",
    filter,
    ...nextestArgs,
  ]);
  process.exit(code);
}

main();
