#!/usr/bin/env bun
/**
 * Configure the GitHub Actions secrets used by the release workflow
 * (.github/workflows/release.yml) — crates.io and PyPI tokens, plus the
 * nuget.org username for NuGet's OIDC login.
 *
 * npm and NuGet need NO long-lived tokens: the @mediaway org authenticates
 * npm publishes with OIDC Trusted Publishing (npmjs.com → org Settings →
 * Trusted Publishing → this repo), and nuget.org uses a Trusted Publishing
 * policy (nuget.org → Trusted Publishing → repo nyxways/mediaway, workflow
 * file release.yml) with NuGet/login@v1 exchanging the workflow's id-token
 * for a 1-hour API key. Only the nuget.org *username* is stored (NUGET_USER).
 *
 * Interactive menu (TTY): shows which secrets are already set, prints where to
 * obtain each token, and lets you set or delete individual secrets — only the
 * ones you pick. Non-TTY flags for scripting / CI:
 *
 *   bun tools/scripts/release-secrets.ts [--repo owner/repo] --list
 *   bun tools/scripts/release-secrets.ts [--repo owner/repo] --set NAME [VALUE|--stdin]
 *   bun tools/scripts/release-secrets.ts [--repo owner/repo] --delete NAME
 *   bun tools/scripts/release-secrets.ts --help
 *
 * Requires the GitHub CLI (`gh`) installed, authenticated, and with access to
 * the repository. Secrets are stored at the repository level (Settings →
 * Secrets and variables → Actions), which is exactly what the release
 * workflow reads.
 *
 * GITHUB_TOKEN needs no setup — Actions provides it automatically.
 */

import { createInterface, type Interface } from "node:readline";
import { Writable } from "node:stream";

interface SecretSpec {
  name: string;
  registry: string;
  url: string;
  steps: string[];
  note?: string;
}

const SECRETS: SecretSpec[] = [
  {
    name: "CARGO_REGISTRY_TOKEN",
    registry: "crates.io",
    url: "https://crates.io/settings/tokens",
    steps: [
      "Sign in → API Tokens → New token.",
      "Scopes: publish-new (first release of new crates) AND publish-update (later versions).",
      "The token is shown only once at creation — copy it immediately.",
    ],
    note: "Used by the release workflow's `crates` job (cargo publish).",
  },
  {
    name: "NUGET_USER",
    registry: "nuget.org",
    url: "https://www.nuget.org/account",
    steps: [
      "Your nuget.org account name (shown at nuget.org — click your avatar). Not a token.",
      "Used by NuGet/login@v1 to exchange the workflow's OIDC id-token for a 1-hour API key (Trusted Publishing policy: repo nyxways/mediaway, workflow file release.yml).",
    ],
    note: "No long-lived NuGet API key is stored.",
  },
  {
    name: "PYPI_TOKEN",
    registry: "PyPI",
    url: "https://pypi.org/manage/account/token/",
    steps: [
      "Add API token → Scope: entire account, or project `mediaway` only (recommended).",
      "The upload username is `__token__` — the release workflow sets it automatically.",
    ],
    note: "Uploads the `mediaway` wheel.",
  },
];

const GITHUB_TOKEN_NOTE =
  "GITHUB_TOKEN is automatic (Actions provides it); it creates the v<version> tag and the GitHub release — nothing to set.";

const NPM_OIDC_NOTE =
  "npm needs no token: @mediaway uses OIDC Trusted Publishing (org Settings → Trusted Publishing on npmjs.com).";

const NUGET_OIDC_NOTE =
  "NuGet needs no API key: nuget.org Trusted Publishing policy + NuGet/login@v1 (OIDC) — only NUGET_USER (a username) is stored.";

const C_RESET = "\x1b[0m";
const C_BOLD = "\x1b[1m";
const C_DIM = "\x1b[2m";
const C_GREEN = "\x1b[32m";
const C_YELLOW = "\x1b[33m";
const C_RED = "\x1b[31m";
const C_CYAN = "\x1b[36m";
const useColor = !!process.stdout.isTTY;

const color = (code: string, s: string): string => (useColor ? `${code}${s}${C_RESET}` : s);

interface CliOptions {
  repo?: string;
  mode: "interactive" | "list" | "set" | "delete" | "help";
  name?: string;
  value?: string;
}

function parseArgs(argv: string[]): CliOptions {
  const opts: CliOptions = { mode: "interactive" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    switch (a) {
      case "--repo":
        opts.repo = argv[++i];
        break;
      case "--list":
        opts.mode = "list";
        break;
      case "--set":
        opts.mode = "set";
        opts.name = argv[++i];
        break;
      case "--delete":
        opts.mode = "delete";
        opts.name = argv[++i];
        break;
      case "--stdin":
        opts.value = "";
        break;
      case "--help":
      case "-h":
        opts.mode = "help";
        break;
      default:
        if (opts.mode === "set" && opts.value === undefined) opts.value = a;
    }
  }
  if (opts.mode === "set" && !opts.name) {
    console.error("error: --set requires a secret name (see --help)");
    process.exit(2);
  }
  return opts;
}

function specFor(name: string): SecretSpec | undefined {
  return SECRETS.find((s) => s.name === name);
}

// --- gh plumbing ----------------------------------------------------------

async function runGh(args: string[]): Promise<{ code: number; stdout: string; stderr: string }> {
  const proc = Bun.spawn(["gh", ...args], { stdout: "pipe", stderr: "pipe" });
  const [stdout, stderr] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
  ]);
  const code = await proc.exited;
  return { code, stdout, stderr };
}

async function requireGh(): Promise<void> {
  const v = await runGh(["--version"]);
  if (v.code !== 0) {
    console.error(
      color(C_RED, "error: GitHub CLI (`gh`) not found or not runnable.") +
        "\n  Install it: https://cli.github.com/  (Windows: `winget install GitHub.cli` / `scoop install gh`)",
    );
    process.exit(1);
  }
  const auth = await runGh(["auth", "status"]);
  if (auth.code !== 0) {
    console.error(color(C_RED, "error: `gh` is not authenticated.") + "\n  Run: gh auth login");
    process.exit(1);
  }
}

async function resolveRepo(explicit?: string): Promise<string> {
  if (explicit) return explicit;
  const r = await runGh(["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"]);
  if (r.code !== 0 || !r.stdout.trim()) {
    console.error(
      color(C_RED, "error: cannot resolve the repository.") +
        "\n  Run inside the mediaway checkout, or pass --repo owner/repo.",
    );
    process.exit(1);
  }
  return r.stdout.trim();
}

async function listSecrets(repo: string): Promise<Map<string, string>> {
  const r = await runGh(["secret", "list", "--repo", repo]);
  if (r.code !== 0) {
    console.error(color(C_RED, `error: gh secret list failed:\n${r.stderr.trim()}`));
    process.exit(1);
  }
  const secrets = new Map<string, string>();
  for (const line of r.stdout.split(/\r?\n/)) {
    const [name, updated] = line.split(/\s+/);
    if (name && updated) secrets.set(name, updated);
  }
  return secrets;
}

async function setSecret(repo: string, name: string, value: string): Promise<void> {
  const proc = Bun.spawn(["gh", "secret", "set", name, "--repo", repo], {
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  });
  proc.stdin.write(value);
  proc.stdin.end();
  const code = await proc.exited;
  if (code !== 0) {
    const err = await new Response(proc.stderr).text();
    console.error(color(C_RED, `error: gh secret set ${name} failed:\n${err.trim()}`));
    process.exit(1);
  }
}

async function deleteSecret(repo: string, name: string): Promise<void> {
  const r = await runGh(["secret", "delete", name, "--repo", repo]);
  if (r.code !== 0) {
    console.error(color(C_RED, `error: gh secret delete ${name} failed:\n${r.stderr.trim()}`));
    process.exit(1);
  }
}

// --- prompts --------------------------------------------------------------

async function prompt(rl: Interface, question: string, hidden = false): Promise<string> {
  return new Promise((resolve) => {
    if (hidden) {
      // Write the prompt directly (bypasses the muted wrapper), then blank-
      // question: readline echoes keystrokes through `output`, which the
      // caller has muted — input never appears on screen.
      process.stdout.write(question);
      rl.question("", (answer) => resolve(answer.trim()));
    } else {
      rl.question(question, (answer) => resolve(answer.trim()));
    }
  });
}

function makeReadline() {
  let muted = false;
  const output = new Writable({
    write(chunk: Buffer, _enc: BufferEncoding, cb: (err?: Error | null) => void) {
      if (!muted) process.stdout.write(chunk);
      cb();
    },
  });
  return { rl: createInterface({ input: process.stdin, output, terminal: true }), setMuted: (m: boolean) => (muted = m) };
}

// --- rendering ------------------------------------------------------------

function printSpec(spec: SecretSpec): void {
  console.log();
  console.log(color(C_BOLD, spec.name) + color(C_DIM, `  — ${spec.registry} token`));
  console.log(`  Get it at: ${color(C_CYAN, spec.url)}`);
  for (const step of spec.steps) console.log(`    • ${step}`);
  if (spec.note) console.log(color(C_DIM, `  ${spec.note}`));
}

function renderStatus(secrets: Map<string, string>): void {
  console.log();
  for (const spec of SECRETS) {
    const existing = secrets.get(spec.name);
    const status = existing
      ? color(C_GREEN, `[set] ${existing}`)
      : color(C_RED, "[not set]");
    console.log(`  ${spec.name.padEnd(22)} ${spec.registry.padEnd(10)} ${status}`);
  }
  console.log(color(C_DIM, `  ${NPM_OIDC_NOTE}`));
  console.log(color(C_DIM, `  ${NUGET_OIDC_NOTE}`));
  console.log(color(C_DIM, `  ${GITHUB_TOKEN_NOTE}`));
}

function printHelp(): void {
  console.log(`Mediaway release secrets — set GitHub Actions secrets for the release workflow.

Secrets:
${SECRETS.map((s) => `  ${s.name.padEnd(22)} ${s.registry.padEnd(10)} ${s.url}`).join("\n")}

Usage:
  bun tools/scripts/release-secrets.ts                interactive menu (TTY)
  bun tools/scripts/release-secrets.ts --list         show set/not-set status
  bun tools/scripts/release-secrets.ts --set NAME VALUE
  bun tools/scripts/release-secrets.ts --set NAME --stdin   (read value from stdin)
  bun tools/scripts/release-secrets.ts --delete NAME
  bun tools/scripts/release-secrets.ts --repo owner/repo ...   (skip repo detection)

${NPM_OIDC_NOTE}
${NUGET_OIDC_NOTE}
${GITHUB_TOKEN_NOTE}`);
}

// --- interactive menu -----------------------------------------------------

async function configureOne(
  repo: string,
  spec: SecretSpec,
  secrets: Map<string, string>,
  rl: import("node:readline").Interface,
  setMuted: (m: boolean) => void,
): Promise<void> {
  printSpec(spec);
  const existing = secrets.get(spec.name);
  if (existing) {
    const act = (await prompt(rl, `  Already set (${existing}). [s]et new value, [d]elete, [c]ancel: `)).toLowerCase();
    if (act === "d") {
      await deleteSecret(repo, spec.name);
      console.log(color(C_GREEN, `  ✓ deleted ${spec.name}`));
      return;
    }
    if (act !== "s") return;
  }
  setMuted(true);
  const value = await prompt(rl, "  Paste the token (input hidden): ", true);
  setMuted(false);
  if (!value) {
    console.log(color(C_YELLOW, "  empty value — cancelled"));
    return;
  }
  await setSecret(repo, spec.name, value);
  console.log(color(C_GREEN, `  ✓ set ${spec.name}`));
}

async function interactive(repo: string): Promise<void> {
  const { rl, setMuted } = makeReadline();
  try {
    for (;;) {
      const secrets = await listSecrets(repo);
      renderStatus(secrets);
      const choice = (
        await prompt(rl, "\n  Select a secret to configure (1-" + SECRETS.length + ", a = all not set, q = quit): ")
      ).toLowerCase();
      if (choice === "q" || choice === "") {
        console.log("  bye");
        return;
      }
      if (choice === "a") {
        for (const spec of SECRETS) {
          if (!secrets.has(spec.name)) await configureOne(repo, spec, secrets, rl, setMuted);
        }
        continue;
      }
      const idx = Number(choice) - 1;
      if (Number.isInteger(idx) && idx >= 0 && idx < SECRETS.length) {
        await configureOne(repo, SECRETS[idx], secrets, rl, setMuted);
      } else {
        console.log(color(C_YELLOW, `  unknown choice: ${choice}`));
      }
    }
  } finally {
    rl.close();
  }
}

// --- main -----------------------------------------------------------------

async function main(): Promise<void> {
  const opts = parseArgs(process.argv.slice(2));

  if (opts.mode === "help") {
    printHelp();
    return;
  }

  await requireGh();
  const repo = await resolveRepo(opts.repo);

  if (opts.mode === "list") {
    const secrets = await listSecrets(repo);
    console.log(color(C_BOLD, `Mediaway release secrets — ${repo}`));
    renderStatus(secrets);
    return;
  }

  if (opts.mode === "set") {
    const spec = specFor(opts.name!);
    if (spec) {
      printSpec(spec);
    } else {
      console.log(color(C_YELLOW, `  note: ${opts.name} is not a release-workflow secret — setting it anyway.`));
    }
    let value = opts.value;
    if (value === "") {
      // --stdin: read the value from stdin (scripting)
      value = (await Bun.stdin.text()).trimEnd();
    } else if (value === undefined) {
      const { rl, setMuted } = makeReadline();
      setMuted(true);
      value = await prompt(rl, `  Paste value for ${opts.name} (input hidden): `, true);
      setMuted(false);
      rl.close();
    }
    if (!value) {
      console.error("error: no value provided (pass it as an argument or with --stdin)");
      process.exit(2);
    }
    await setSecret(repo, opts.name!, value);
    console.log(color(C_GREEN, `✓ set ${opts.name}`));
    return;
  }

  if (opts.mode === "delete") {
    await deleteSecret(repo, opts.name!);
    console.log(color(C_GREEN, `✓ deleted ${opts.name}`));
    return;
  }

  // interactive
  if (!process.stdin.isTTY) {
    console.error(
      color(C_RED, "error: interactive mode needs a TTY.") +
        "\n  Use --list / --set / --delete for scripting (see --help).",
    );
    process.exit(2);
  }
  console.log(color(C_BOLD, `Mediaway release secrets — ${repo}`));
  await interactive(repo);
}

await main();
