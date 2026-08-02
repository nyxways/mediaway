# Dev scripts (Bun + TypeScript)

Light **repository utilities** (codegen helpers, fixture/check glue, one-off maintainers’ tools that are not Rust product code) are written in **TypeScript** and run with **[Bun](https://bun.sh/)**.

Canonical layout: `tools/scripts/`.

## When to use what

| Need | Use |
|------|-----|
| Library / facade / platform / sans-io / product CLIs (`mediaway-*-cli`) | **Rust** (`crates/`, `tools/mediaway-*-cli`) |
| New light maintainers’ utilities | **Bun + TypeScript** (`tools/scripts/`) |
| Git hooks invoked by lefthook | Keep **`tools/hooks/*.sh`** (bash) — no Bun required for every commit |
| Machine-local throwaways | `local/` (gitignored) — any language; do not commit |

Do **not** add new Node.js, Python, PowerShell, or Ruby utility trees for shared repo scripts. Prefer Bun TS. Exceptions need a short note in the PR (or an ADR if it becomes standing policy).

## Conventions

- **Runtime:** Bun (pin a version in `tools/scripts/package.json` / docs when the first real script lands).
- **Language:** TypeScript (`strict`). Prefer `.ts` entrypoints invoked as `bun run …` / `bun tools/scripts/…`.
- **Deps:** Same care as Cargo — deliberate adds, permissive licenses; do not pull a second JS package manager as the default workflow.
- **Scope:** Scripts must not become a parallel product API. Shipping Mediaway remains Rust (+ planned `*-ffi` / WASM tracks).
- **Size:** Source files ≤1000 lines ([code-style.md](code-style.md)).
- **English** comments and user-facing script messages (Language policy).

## Setup (when scripts exist)

```bash
# https://bun.sh
bun --version
cd tools/scripts && bun install   # once package.json exists
bun run <script>
```

Bun is **optional** for contributors who only touch Rust, until a task requires a script under `tools/scripts/`.

## Related

- Tree: [repo-structure.md](repo-structure.md)
- Hooks stay bash: [hooks.md](hooks.md)
- Agent tools overview: [`docs/ai/tools.md`](../ai/tools.md)
