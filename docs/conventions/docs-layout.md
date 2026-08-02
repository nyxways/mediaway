# Documentation layout

## Audience

| Surface | Audience | Put here |
|---------|----------|----------|
| Root `README.md`, `CONTRIBUTING.md`, most of `docs/contributing/` | **Humans** | Status, setup, how to contribute |
| `docs/contributing/for-agents.md` | **Contributor AIs** | Onboarding brief → `AGENTS.md` |
| Root / crate `README.md` | **Humans** | What the project/crate is |
| `docs/book/` | **Humans** (rendered as a GitHub Pages site) | Getting-started guides, per-capability tutorials — narrative onboarding, not SSOT |
| `docs/spec/`, crate `docs/roadmap.md`, `adr/` | Humans + agents (engineering) | Design, stages, decisions — **no agent workflow prose** |
| `AGENTS.md`, `docs/ai/`, `docs/ai/wiki/` | **All agents** | Rules, Rule 0, wiki, tooling entrypoints |
| `local/` (gitignored) | Humans + agents (this machine) | Scratch memory / experiments — **not** SSOT |

**Do not** put agent instructions, wiki “read first” notes, or AGENTS pointers in human READMEs or `CONTRIBUTING.md`. Agents enter via `AGENTS.md` → wiki, not via `README.md`. Use [`local/`](../../local/README.md) for machine-only notes.

## Workspace (`docs/`)

Cross-cutting only:

| Path | Role |
|------|------|
| `docs/contributing/` | Human contributor guides |
| `docs/conventions/` | Commits, hooks, style, security, testing, benchmarking |
| `docs/benchmarks/` | Reference machine profiles + shared bench notes |
| `docs/ai/` | Agent stack/layout/tools + **wiki** (Rule 0) |
| `docs/spec/` | High-level product / pipeline SSOT |
| `docs/roadmap.md` | Platform order + **index of crate roadmaps** |
| `docs/adr/` | **Workspace-wide** ADRs only |
| Root `README.md` | Human entry + **codec support tables** (OS / GPU / CPU) |
| `docs/book/` | mdBook user guide source, deployed to GitHub Pages by `.github/workflows/docs.yml`. Reference pages pull the README's support tables via `{{#include README.md:codec-support}}`-style anchors instead of duplicating them — keep those `<!-- ANCHOR: ... -->` comments in the README in sync if you move/rename a section. Guides are hand-written, not `{{#include}}`d from `examples/` — book code stays teaching-focused/annotated; `examples/` stays the compiling, tested source of truth. |

## Per crate (`crates/<name>/`, `tools/<name>/`)

```
crates/mediaway-encoder/
├── README.md          # short human/engineering overview (not agent ops)
├── docs/
│   └── roadmap.md     # this crate’s stages
└── adr/
    ├── README.md
    └── template.md
```

### Rules

1. **Crate work plan → that crate’s `docs/roadmap.md`.**
2. **Crate decisions → that crate’s `adr/`.**
3. **Workspace `docs/roadmap.md`** = platform policy (Windows → Web → Linux → other) + links to crate roadmaps — not a dump of every checkbox.
4. **Agents:** wiki (Rule 0) → `docs/spec/` as needed → crate `docs/roadmap.md` + `adr/` (skip root/`README.md` for agent workflow).
5. English prose. ADR numbers are per `adr/` folder.
6. Wiki stays short; detail in crate `docs/` / `adr/` / `docs/spec/`.
