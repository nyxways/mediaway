# Conventions: local workspace

Developers and agents may use a **gitignored** tree at repo root:

| Path | Role |
|------|------|
| [`local/README.md`](../../local/README.md) | Tracked guide |
| `local/agent/`, `local/machine/`, `local/experiments/`, … | Ignored content |

**In git:** only `local/README.md` + `local/.gitignore`.  
**Not for:** secrets to share, ADRs, wiki SSOT, workspace crates, **full external standards** (those stay under `local/standards/` if downloaded — [`external-standards.md`](external-standards.md)).

Root `.gitignore` also lists `/local/**` with exceptions for those two files.
