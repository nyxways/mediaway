# Repo structure

```
mediaway/
├── README.md
├── CONTRIBUTING.md
├── AGENTS.md                 # agents only — not a human entry
├── CLAUDE.md
├── Cargo.toml
├── crates/
│   └── mediaway-<role>/      # facade or sans-io core
│       ├── src/
│       ├── docs/
│       └── adr/
│   └── mediaway-<role>-<os>/ # platform backends (when added)
├── tools/
│   ├── hooks/                # lefthook bash gates
│   ├── scripts/              # Bun + TypeScript utilities (see conventions/scripts.md)
│   └── mediaway-*-cli/       # Rust product CLIs (not library API)
├── docs/
│   ├── contributing/         # human contributor guides
│   ├── conventions/
│   ├── ai/wiki/              # agents
│   ├── adr/
│   ├── spec/
│   └── roadmap.md
├── local/                    # gitignored scratch (README tracked)
└── …
```

**Local workspace:** [`local/README.md`](../../local/README.md) · [`local-workspace.md`](local-workspace.md) — agent/machine memory and experiments; never commit contents.

Details: [`docs-layout.md`](docs-layout.md) · packaging: [`../spec/crate-packaging.md`](../spec/crate-packaging.md).

## Crate naming (v1)

| Kind | Pattern |
|------|---------|
| Reusable domain core (no Mediaway types) | Unprefixed — e.g. `iso-bmff`, `iso-cenc` ([ADR-0012](../adr/0012-unprefixed-reusable-cores.md)) |
| Shared / facade | `mediaway-<name>` under `crates/` |
| Container | `iso-bmff` + `mediaway-container` (facade/`mp4`) |
| Platform backend | `mediaway-<capability>-<platform>` |
| CLI tools | `tools/mediaway-*` |
| Light scripts | `tools/scripts/` — Bun + TypeScript ([scripts.md](scripts.md)) |


## Feature flags (facades)

- `default` = portable traits / no heavy backends
- Optional features may pull one backend crate (`windows`, `web`, …) — never all platforms by default
