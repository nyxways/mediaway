# Claude Code / agent docs (Mediaway)

Rules live in **`AGENTS.md` (SSOT)**. This file describes layout only.

## Language

- **Chat with the user:** user's language
- **Repo artifacts** (including this file, wiki, ADRs, issues, commits): **English only**

## Session load order

1. `docs/ai/wiki/index.md` (+ related wiki) — **Rule 0**
2. `AGENTS.md` (via `CLAUDE.md` `@AGENTS.md`)
3. Crate `docs/` + `adr/` when touching that crate
4. `docs/spec/` · workspace `docs/adr/` · `docs/conventions/` (as needed)

## Layout

```
AGENTS.md
docs/                         ← workspace-only
  conventions/docs-layout.md  ← where crate vs workspace docs go
  ai/wiki/
  adr/                        ← workspace ADRs only
crates/<name>/{docs,adr}/     ← per-crate design + ADRs
tools/<name>/{docs,adr}/
```

## Wiki (summary)

- Before work: index → related pages
- After work: update / create + category index link
- 100-line limit (hook); deep detail → crate `docs/`
- English prose
