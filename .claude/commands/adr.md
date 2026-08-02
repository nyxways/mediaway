---
description: Create a new ADR. Default crate-local; use workspace for cross-cutting only.
argument-hint: <crate-or-workspace> <short-title>
---

Create a new ADR.

## Arguments

- `$1` = crate path folder name **or** `workspace`  
  Examples: `mediaway-encoder`, `mediaway-common`, `mediaway-avcli`, `workspace`
- `$2` = short title (English)

If only one argument is given, treat it as the title and ask which crate (or `workspace`).

## Steps

### Crate-local (default)

1. Resolve directory:
   - libs: `crates/$1/adr/`
   - CLIs: `tools/$1/adr/` (ffmpeg/ffprobe)
2. Highest `NNNN` in that `adr/` → next = +1
3. Path: `…/adr/<NNNN>-<kebab-title>.md`
4. Copy that folder’s `template.md`
5. Update that folder’s `adr/README.md`
6. Optionally add a one-line pointer under the crate `docs/roadmap.md` or wiki (not root/`README.md` agent prose)

### Workspace

1. Only for cross-cutting policy (tooling, monorepo, shared license rules)
2. Use `docs/adr/` the same way
3. See `docs/conventions/docs-layout.md`

## Rules

- English body
- Never reuse numbers **within that adr folder**
- Supersede with a new ADR + old Status `Superseded by ADR-NNNN`
- Chat with the user in their language
