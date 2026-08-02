/**
 * Fetch / pin / verify external standards under local/standards/
 * against docs/standards/registry.toml (BLAKE3).
 *
 * Usage:
 *   bun tools/scripts/fetch-standard.ts [--ai-agent] <id>
 *   bun tools/scripts/fetch-standard.ts [--ai-agent] verify <id>
 *   bun tools/scripts/fetch-standard.ts [--ai-agent] pin <id>
 *
 * --ai-agent  → User-Agent discloses an AI coding agent
 * (default)   → User-Agent discloses a human maintainer
 *
 * The Mediaway-standards-fetch User-Agent is for THIS script only.
 * Do not reuse it on other HTTP clients or product code.
 */

import { blake3 } from "@noble/hashes/blake3";
import { bytesToHex } from "@noble/hashes/utils";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

type StandardEntry = {
  id: string;
  url: string;
  filename: string;
  blake3: string;
  paywalled?: boolean;
  note?: string;
};

type Actor = "human" | "ai-agent";

const UA = {
  human: "Mediaway-standards-fetch/0.1 (+https://github.com/nyxways/mediaway; human-maintainer)",
  "ai-agent":
    "Mediaway-standards-fetch/0.1 (+https://github.com/nyxways/mediaway; ai-coding-agent)",
} as const;

function repoRoot(): string {
  return path.resolve(import.meta.dir, "../..");
}

/** Minimal [[standard]] TOML table parser (flat string/bool fields only). */
function parseRegistry(text: string): StandardEntry[] {
  const entries: StandardEntry[] = [];
  let cur: Partial<StandardEntry> | null = null;

  const flush = () => {
    if (!cur) return;
    if (!cur.id || !cur.url || !cur.filename) {
      throw new Error(`Incomplete [[standard]] block near id=${cur.id ?? "?"}`);
    }
    entries.push({
      id: cur.id,
      url: cur.url,
      filename: cur.filename,
      blake3: cur.blake3 ?? "",
      paywalled: cur.paywalled ?? false,
      note: cur.note,
    });
    cur = null;
  };

  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    if (line === "[[standard]]") {
      flush();
      cur = {};
      continue;
    }
    if (!cur) continue;
    const m = /^(\w+)\s*=\s*(.+)$/.exec(line);
    if (!m) continue;
    const key = m[1];
    let val = m[2].trim();
    if (val === "true" || val === "false") {
      (cur as Record<string, unknown>)[key] = val === "true";
      continue;
    }
    if (
      (val.startsWith('"') && val.endsWith('"')) ||
      (val.startsWith("'") && val.endsWith("'"))
    ) {
      val = val.slice(1, -1);
    }
    (cur as Record<string, string>)[key] = val;
  }
  flush();
  return entries;
}

function digestFile(buf: Uint8Array): string {
  return bytesToHex(blake3(buf));
}

async function loadRegistry(): Promise<StandardEntry[]> {
  const p = path.join(repoRoot(), "docs/standards/registry.toml");
  const text = await readFile(p, "utf8");
  return parseRegistry(text);
}

function findEntry(entries: StandardEntry[], id: string): StandardEntry {
  const hit = entries.find((e) => e.id === id);
  if (!hit) {
    throw new Error(`Unknown standard id "${id}". Add it to docs/standards/registry.toml`);
  }
  return hit;
}

function localDir(id: string): string {
  return path.join(repoRoot(), "local/standards", id);
}

function userAgent(actor: Actor): string {
  return UA[actor];
}

async function verify(entry: StandardEntry): Promise<void> {
  const filePath = path.join(localDir(entry.id), entry.filename);
  const buf = await readFile(filePath);
  const got = digestFile(buf);
  console.log(`blake3(${entry.filename}) = ${got}`);

  if (!entry.blake3) {
    console.log(
      "Registry blake3 is empty — pin it with: bun tools/scripts/fetch-standard.ts pin " +
        entry.id,
    );
    return;
  }
  if (got !== entry.blake3.toLowerCase()) {
    throw new Error(
      `BLAKE3 mismatch for ${entry.id}:\n  expected ${entry.blake3}\n  got      ${got}\nRefusing to trust this file.`,
    );
  }
  console.log("OK — matches docs/standards/registry.toml");
}

async function pin(entry: StandardEntry): Promise<void> {
  const filePath = path.join(localDir(entry.id), entry.filename);
  const buf = await readFile(filePath);
  const got = digestFile(buf);
  console.log(`File: ${filePath}`);
  console.log(`blake3 = "${got}"`);
  console.log("Commit this into docs/standards/registry.toml for id=" + entry.id);
}

async function fetchAndVerify(entry: StandardEntry, actor: Actor): Promise<void> {
  const dir = localDir(entry.id);
  await mkdir(dir, { recursive: true });
  await writeFile(path.join(dir, "source.url"), `${entry.url}\n`, "utf8");
  await writeFile(path.join(dir, "source.ua"), `${userAgent(actor)}\n`, "utf8");

  if (entry.paywalled) {
    console.log(`paywalled=true — will not download ${entry.id}.`);
    console.log(`Place a lawful copy at: ${path.join(dir, entry.filename)}`);
    console.log(`Then: bun tools/scripts/fetch-standard.ts pin ${entry.id}`);
    console.log(`  or: bun tools/scripts/fetch-standard.ts verify ${entry.id}`);
    return;
  }

  const ua = userAgent(actor);
  console.log(`GET ${entry.url}`);
  console.log(`User-Agent: ${ua}`);
  const res = await fetch(entry.url, {
    headers: {
      "User-Agent": ua,
      Accept: "*/*",
    },
  });
  if (!res.ok) {
    throw new Error(`HTTP ${res.status} fetching ${entry.url}`);
  }
  const buf = new Uint8Array(await res.arrayBuffer());
  const dest = path.join(dir, entry.filename);
  await writeFile(dest, buf);
  console.log(`Wrote ${dest} (${buf.byteLength} bytes)`);
  await verify(entry);
}

function parseArgs(argv: string[]): {
  mode: "fetch" | "verify" | "pin";
  id: string;
  actor: Actor;
} {
  const flags = new Set(argv.filter((a) => a.startsWith("--")));
  const positional = argv.filter((a) => !a.startsWith("--"));

  if (flags.has("--help") || flags.has("-h")) {
    return { mode: "fetch", id: "", actor: "human" };
  }

  const actor: Actor = flags.has("--ai-agent") ? "ai-agent" : "human";
  let mode: "fetch" | "verify" | "pin" = "fetch";
  let id = "";

  if (positional[0] === "verify" || positional[0] === "pin") {
    mode = positional[0];
    id = positional[1] ?? "";
  } else {
    id = positional[0] ?? "";
  }

  return { mode, id, actor };
}

async function main(): Promise<void> {
  const { mode, id, actor } = parseArgs(process.argv.slice(2));

  if (!id) {
    console.error(`Usage:
  bun tools/scripts/fetch-standard.ts [--ai-agent] <id>
  bun tools/scripts/fetch-standard.ts [--ai-agent] verify <id>
  bun tools/scripts/fetch-standard.ts [--ai-agent] pin <id>

  --ai-agent   User-Agent discloses ai-coding-agent (agents must pass this on THIS tool)
  (default)    User-Agent discloses human-maintainer
  Do not use Mediaway-standards-fetch UA outside this script.`);
    process.exit(2);
  }

  const entries = await loadRegistry();
  const entry = findEntry(entries, id);

  if (mode === "verify") await verify(entry);
  else if (mode === "pin") await pin(entry);
  else await fetchAndVerify(entry, actor);
}

main().catch((err: unknown) => {
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
});
