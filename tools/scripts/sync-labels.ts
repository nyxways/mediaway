#!/usr/bin/env bun
/**
 * Apply the Mediaway issue-label set to the GitHub repo (idempotent).
 *
 * Usage:
 *   gh auth login            # once
 *   bun tools/scripts/sync-labels.ts          # apply/update all labels
 *   bun tools/scripts/sync-labels.ts --dry    # print what would change
 *
 * Label list is the SSOT for docs/conventions/issues.md § Labels.
 * Colors: rough GitHub palette per category (kind/area/language/platform/state).
 */

interface Label {
  name: string;
  description: string;
  color: string;
}

const LABELS: Label[] = [
  // ── kind ──────────────────────────────────────────────────────────────────
  { name: "bug", description: "Incorrect behavior", color: "d73a4a" },
  { name: "crash", description: "Panic, abort, hang, deadlock", color: "b60205" },
  { name: "docs", description: "Documentation", color: "0075ca" },
  { name: "enhancement", description: "Feature / improvement", color: "a2eeef" },
  { name: "design", description: "API / ADR / packaging discussion", color: "d4c5f9" },
  { name: "perf", description: "Performance / vectorization / alloc", color: "fbca04" },
  { name: "security", description: "Security vulnerability (report privately per SECURITY.md)", color: "b60205" },
  // ── area ──────────────────────────────────────────────────────────────────
  { name: "area:core", description: "mediaway-common, sans-io cores (iso-bmff, iso-cenc, riff_wave, ...)", color: "0e8a16" },
  { name: "area:container", description: "mux/demux + container-ffi", color: "0e8a16" },
  { name: "area:encoder", description: "Encoder crates + encoder-ffi", color: "0e8a16" },
  { name: "area:decoder", description: "Decoder crates + decoder-ffi", color: "0e8a16" },
  { name: "area:device", description: "Device capture crates + device-ffi", color: "0e8a16" },
  { name: "area:bindings", description: "Cross-language binding or packaging work", color: "5319e7" },
  // ── binding language ──────────────────────────────────────────────────────
  { name: "binding:rust", description: "Rust API surface (examples, docs, API shape)", color: "c5def5" },
  { name: "binding:c", description: "C binding (bindings/c)", color: "c5def5" },
  { name: "binding:cpp", description: "C++ binding (bindings/cpp)", color: "c5def5" },
  { name: "binding:csharp", description: "C# binding (bindings/csharp)", color: "c5def5" },
  { name: "binding:python", description: "Python binding (bindings/python)", color: "c5def5" },
  { name: "binding:node", description: "Node.js binding (bindings/nodejs)", color: "c5def5" },
  { name: "binding:browser", description: "Browser/WASM binding (bindings/browser)", color: "c5def5" },
  // ── platform ──────────────────────────────────────────────────────────────
  { name: "platform:windows", description: "Windows", color: "bfd4f2" },
  { name: "platform:web", description: "Web / WASM", color: "bfd4f2" },
  { name: "platform:linux", description: "Linux", color: "bfd4f2" },
  { name: "platform:apple", description: "macOS / iOS", color: "bfd4f2" },
  { name: "platform:android", description: "Android", color: "bfd4f2" },
  // ── state / priority ──────────────────────────────────────────────────────
  { name: "state:needs triage", description: "New inbound", color: "ededed" },
  { name: "good first issue", description: "Small, well-scoped", color: "7057ff" },
  { name: "priority:high", description: "Urgent: data loss, security, red CI, broken released package", color: "e99695" },
];

const dry = process.argv.includes("--dry");

for (const label of LABELS) {
  const args = [
    "label", "create", label.name,
    "--description", label.description,
    "--color", label.color,
    "--force",
  ];
  if (dry) {
    console.log(`[dry] gh ${args.join(" ")}`);
    continue;
  }
  const { stdout, exitCode } = await Bun.spawn(["gh", ...args]).exited.then(
    () => Bun.spawnSync(["gh", ...args]),
  );
  if (exitCode !== 0) {
    console.error(`gh label create failed for ${label.name}: ${stdout}`);
    process.exitCode = 1;
  } else {
    console.log(`label ${label.name} ✓`);
  }
}

if (dry) {
  console.log(`[dry] ${LABELS.length} labels would be applied`);
} else {
  console.log(`applied ${LABELS.length} labels`);
}
