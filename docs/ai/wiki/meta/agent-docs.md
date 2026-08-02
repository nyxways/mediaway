# Agent doc layout

| File | Role |
|------|------|
| `AGENTS.md` | Rules SSOT (edit here only) |
| `CLAUDE.md` | `@AGENTS.md` import |
| `.agents/rules/main.md` | Other-tool always-on pointer |
| `docs/ai/stack.md` · `layout.md` · `tools.md` | Short references |
| `docs/ai/wiki/` | Cross-session knowledge (required before work) |
| `docs/conventions/` | Commits, hooks, style (humans + agents) |
| `docs/spec/` · `docs/adr/` | Design and decisions (incl. [zca](zca.md) / ADR-0009) |

**Not agent surfaces:** root `README.md`, `CONTRIBUTING.md`, `docs/contributing/` — do not put Rule 0 / wiki / AGENTS prose there. See [docs-layout](docs-layout.md).

**Local scratch:** [`local/`](../../../../local/README.md) — gitignored memory/experiments; not SSOT.

Wiki: before work → `wiki/index.md` → related pages. After work → update. **≤100 lines/file. English.**
