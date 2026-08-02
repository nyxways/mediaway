/**
 * Download listed FFmpeg FATE samples via HTTP into `local/.cache/fate/`.
 *
 * Only paths listed in every crate's `crates/*\/tests/fate_manifest.txt`
 * (not the full multi‑GB suite).
 *
 * Usage:
 *   bun tools/scripts/fetch-fate-samples.ts
 *   bun tools/scripts/fetch-fate-samples.ts --ai-agent
 *   bun tools/scripts/fetch-fate-samples.ts --force
 *
 * Then, per crate:
 *   MEDIAWAY_FATE_SAMPLES=local/.cache/fate cargo test -p iso-bmff --test demux_exceptions
 *   MEDIAWAY_FATE_SAMPLES=local/.cache/fate cargo test -p ebml-webm --test demux_exceptions
 *   ...
 *
 * User-Agent: this script only (`Mediaway-fate-fetch`). Do not reuse on other clients.
 * Agents must pass `--ai-agent`. Humans omit the flag.
 */

import { mkdir, readFile, writeFile, access } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "../..");
const MANIFEST_GLOB = "crates/*/tests/fate_manifest.txt";
const DEST_ROOT = join(REPO_ROOT, "local/.cache/fate");
/** Public HTTP mirror of the FATE suite tree. */
const BASE_URL = "https://fate-suite.ffmpeg.org";

const UA = {
  human: "Mediaway-fate-fetch/0.1 (+https://github.com/nyxways/mediaway; human-maintainer)",
  "ai-agent":
    "Mediaway-fate-fetch/0.1 (+https://github.com/nyxways/mediaway; ai-coding-agent)",
} as const;

type Actor = keyof typeof UA;

function parseArgs(argv: string[]): { actor: Actor; force: boolean } {
  let actor: Actor = "human";
  let force = false;
  for (const a of argv) {
    if (a === "--ai-agent") actor = "ai-agent";
    if (a === "--force") force = true;
  }
  return { actor, force };
}

/** Each manifest line is `path <TAB|space> mode` — only the path is a download target. */
function parseManifest(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.startsWith("#"))
    .map((l) => l.split(/\s+/, 1)[0])
    .filter((p): p is string => p !== undefined && p.length > 0);
}

async function exists(path: string): Promise<boolean> {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function downloadOne(
  rel: string,
  dest: string,
  ua: string,
  force: boolean,
): Promise<"ok" | "skip" | "fail"> {
  if (!force && (await exists(dest))) {
    console.log(`skip (exists): ${rel}`);
    return "skip";
  }
  const url = `${BASE_URL}/${rel.split("\\").join("/")}`;
  console.log(`GET ${url}`);
  const res = await fetch(url, {
    headers: { "User-Agent": ua, Accept: "*/*" },
    redirect: "follow",
  });
  if (!res.ok) {
    console.error(`fail ${rel}: HTTP ${res.status} ${res.statusText}`);
    return "fail";
  }
  const buf = new Uint8Array(await res.arrayBuffer());
  if (buf.byteLength === 0) {
    console.error(`fail ${rel}: empty body`);
    return "fail";
  }
  await mkdir(dirname(dest), { recursive: true });
  await writeFile(dest, buf);
  console.log(`wrote ${rel} (${buf.byteLength} bytes)`);
  return "ok";
}

const { actor, force } = parseArgs(process.argv.slice(2));
const ua = UA[actor];

const manifestFiles = [...new Bun.Glob(MANIFEST_GLOB).scanSync({ cwd: REPO_ROOT })];
if (manifestFiles.length === 0) {
  console.error(`no manifests matched ${MANIFEST_GLOB}`);
  process.exit(1);
}

// De-duplicate paths shared across crate manifests (e.g. none today, but
// harmless if two crates ever reference the same fate-suite file).
const paths = new Set<string>();
for (const manifestFile of manifestFiles.sort()) {
  const text = await readFile(join(REPO_ROOT, manifestFile), "utf8");
  const manifestPaths = parseManifest(text);
  console.log(`manifest ${manifestFile}: ${manifestPaths.length} entries`);
  for (const p of manifestPaths) paths.add(p);
}
if (paths.size === 0) {
  console.error("all manifests are empty");
  process.exit(1);
}

let ok = 0;
let skip = 0;
let fail = 0;
for (const rel of paths) {
  const dest = join(DEST_ROOT, rel);
  const r = await downloadOne(rel, dest, ua, force);
  if (r === "ok") ok += 1;
  else if (r === "skip") skip += 1;
  else fail += 1;
}

console.log(
  `done: ${ok} downloaded, ${skip} skipped, ${fail} failed → ${DEST_ROOT}`,
);
console.log(`Set MEDIAWAY_FATE_SAMPLES=${DEST_ROOT}`);
process.exit(fail > 0 && ok + skip === 0 ? 1 : 0);
