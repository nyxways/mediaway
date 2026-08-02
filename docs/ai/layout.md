# Folder layout — where things live

Exploration map. Deep design lives in **crate `docs/` / `adr/`** and **`docs/spec/`**; wiki stays short.

**Audience:** agent workflow lives in `AGENTS.md` + `docs/ai/` (+ wiki). Humans use `README.md`, `CONTRIBUTING.md`, `docs/contributing/` — see [`docs/conventions/docs-layout.md`](../conventions/docs-layout.md).

## Workspace `docs/`

| Path | Contents |
|------|----------|
| `docs/ai/wiki/` | Agent knowledge (Rule 0) |
| `docs/contributing/` | Human contributor guides |
| `docs/conventions/` | Shared process rules |
| `local/` | Machine/agent scratch (gitignored; README tracked) |
| `docs/adr/` | Workspace-wide ADRs only |
| `docs/spec/` | Product/pipeline overview |
| `docs/roadmap.md` | Windows → Web → Linux → other |

## Every crate / CLI package

```
README.md         # short overview (not agent ops)
docs/roadmap.md
adr/README.md
adr/template.md
```

| Crate | docs |
|-------|------|
| `mediaway-common` | types |
| `mediaway-encoder` / `decoder` / `device` | **facades** (traits) |
| `mediaway-*-windows` / `*-web` / … | **platform** backends (when added) |
| `iso-bmff` / `mediaway-container` | **sans-io** containers (core + facade) |
| `mediaway-sw` | SW codecs |
| `mediaway-test-media` | fixtures |
| `mediaway-avcli` / `mediaway-avprobe` | CLI compat |

Packaging: [`docs/spec/crate-packaging.md`](../spec/crate-packaging.md).
