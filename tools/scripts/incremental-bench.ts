#!/usr/bin/env bun
/**
 * Incremental benches — run `cargo bench -p …` only for packages touched by
 * the git diff (and reverse-deps that declare [[bench]]).
 *
 * Dev-loop only. Do **not** wire into pre-push / PR CI / bench-daily full gate.
 * Daily main tracking stays `.github/workflows/bench-daily.yml`.
 *
 * Usage:
 *   bun run incremental-bench.ts
 *   bun run incremental-bench.ts --since main
 *   bun run incremental-bench.ts --no-deps
 *   bun run incremental-bench.ts -- --bench mux_throughput
 *
 * Prefers `cargo impact --context` for the file list; falls back to `git diff`.
 * Policy: docs/conventions/benchmarking.md § Incremental (dev)
 */

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

function usage(): never {
  console.error(`Usage: bun run incremental-bench.ts [--since <ref>] [--no-deps] [-- <cargo bench args>]

Defaults: --since HEAD
Dev-loop only — not a CI gate.`);
  process.exit(2);
}

function parseArgs(argv: string[]): {
  since: string;
  noDeps: boolean;
  benchArgs: string[];
} {
  let since = "HEAD";
  let noDeps = false;
  const benchArgs: string[] = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--since") {
      const v = argv[++i];
      if (!v) usage();
      since = v;
    } else if (a === "--no-deps") {
      noDeps = true;
    } else if (a === "--") {
      benchArgs.push(...argv.slice(i + 1));
      break;
    } else if (a === "-h" || a === "--help") {
      usage();
    } else {
      benchArgs.push(a);
    }
  }
  return { since, noDeps, benchArgs };
}

type Pkg = {
  name: string;
  manifestDir: string;
  hasBench: boolean;
  depNames: Set<string>;
};

function runCapture(cmd: string, args: string[]): {
  status: number;
  stdout: string;
  stderr: string;
} {
  const r = spawnSync(cmd, args, { encoding: "utf8", shell: true, cwd: REPO_ROOT });
  return {
    status: r.status ?? 1,
    stdout: r.stdout ?? "",
    stderr: r.stderr ?? "",
  };
}

function runInherit(cmd: string, args: string[]): number {
  const r = spawnSync(cmd, args, {
    encoding: "utf8",
    shell: true,
    cwd: REPO_ROOT,
    stdio: "inherit",
  });
  return r.status ?? 1;
}

function cargoImpactAvailable(): boolean {
  const r = spawnSync("cargo", ["impact", "--help"], {
    encoding: "utf8",
    shell: true,
    cwd: REPO_ROOT,
  });
  return r.status === 0;
}

function loadWorkspacePackages(): Map<string, Pkg> {
  const meta = runCapture("cargo", ["metadata", "--format-version", "1", "--no-deps"]);
  if (meta.status !== 0) {
    console.error(meta.stderr || meta.stdout);
    process.exit(1);
  }
  const data = JSON.parse(meta.stdout) as {
    packages: {
      name: string;
      manifest_path: string;
      dependencies: { name: string }[];
    }[];
    workspace_members: string[];
  };

  const members = new Map<string, Pkg>();
  for (const p of data.packages) {
    if (!data.workspace_members.some((m) => m.startsWith(`${p.name} `))) continue;
    const manifestDir = path.dirname(p.manifest_path);
    const rel = path.relative(REPO_ROOT, manifestDir);
    if (rel.startsWith("..")) continue;
    let hasBench = false;
    try {
      hasBench = /^\[\[bench\]\]/m.test(readFileSync(p.manifest_path, "utf8"));
    } catch {
      /* ignore */
    }
    members.set(p.name, {
      name: p.name,
      manifestDir,
      hasBench,
      depNames: new Set(p.dependencies.map((d) => d.name)),
    });
  }
  return members;
}

function packageForFile(file: string, packages: Map<string, Pkg>): string | null {
  const norm = file.replace(/\\/g, "/");
  let best: { name: string; len: number } | null = null;
  for (const p of packages.values()) {
    const dir = path.relative(REPO_ROOT, p.manifestDir).replace(/\\/g, "/");
    if (!dir || dir.startsWith("..")) continue;
    const prefix = dir.endsWith("/") ? dir : `${dir}/`;
    if (norm === dir || norm.startsWith(prefix)) {
      if (!best || dir.length > best.len) best = { name: p.name, len: dir.length };
    }
  }
  return best?.name ?? null;
}

function changedFiles(since: string): string[] {
  if (cargoImpactAvailable()) {
    const r = runCapture("cargo", ["impact", "--since", since, "--context"]);
    if (r.status === 0 && r.stdout.trim()) {
      return r.stdout
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter(Boolean);
    }
  }

  const files = new Set<string>();
  const add = (block: { status: number; stdout: string }) => {
    if (block.status !== 0) return;
    for (const l of block.stdout.split(/\r?\n/)) {
      if (l.trim()) files.add(l.trim());
    }
  };
  add(runCapture("git", ["diff", "--name-only", since]));
  add(runCapture("git", ["diff", "--name-only"]));
  add(runCapture("git", ["diff", "--name-only", "--cached"]));
  return [...files];
}

function expandReverseDeps(seeds: Set<string>, packages: Map<string, Pkg>): Set<string> {
  const reverse = new Map<string, Set<string>>();
  for (const p of packages.values()) {
    for (const d of p.depNames) {
      if (!packages.has(d)) continue;
      let set = reverse.get(d);
      if (!set) {
        set = new Set();
        reverse.set(d, set);
      }
      set.add(p.name);
    }
  }
  const out = new Set(seeds);
  const queue = [...seeds];
  while (queue.length) {
    const n = queue.pop()!;
    for (const child of reverse.get(n) ?? []) {
      if (!out.has(child)) {
        out.add(child);
        queue.push(child);
      }
    }
  }
  return out;
}

function main(): void {
  const { since, noDeps, benchArgs } = parseArgs(process.argv.slice(2));
  console.error(`[1/3] resolve packages since ${since}`);
  const packages = loadWorkspacePackages();
  const files = changedFiles(since);
  if (files.length === 0) {
    console.error("    no changed files — nothing to bench");
    process.exit(0);
  }

  const seed = new Set<string>();
  for (const f of files) {
    const name = packageForFile(f, packages);
    if (name) seed.add(name);
  }
  if (seed.size === 0) {
    console.error("    changes outside workspace packages — nothing to bench");
    process.exit(0);
  }

  const candidates = noDeps ? seed : expandReverseDeps(seed, packages);
  const toBench = [...candidates].filter((n) => packages.get(n)?.hasBench).sort();

  console.error(`    changed packages: ${[...seed].sort().join(", ")}`);
  if (!noDeps) {
    console.error(`    reverse-deps closure: ${[...candidates].sort().join(", ")}`);
  }
  if (toBench.length === 0) {
    console.error("    no [[bench]] targets among impacted packages");
    process.exit(0);
  }

  console.error(`[2/3] packages with benches: ${toBench.join(", ")}`);
  console.error(
    `[3/3] cargo bench ${toBench.map((p) => `-p ${p}`).join(" ")} ${benchArgs.join(" ")}`.trimEnd(),
  );

  const args = ["bench", ...toBench.flatMap((p) => ["-p", p]), ...benchArgs];
  process.exit(runInherit("cargo", args));
}

main();
