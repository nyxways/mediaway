# Docs layout (pointer)

Canonical: [`docs/conventions/docs-layout.md`](../../../conventions/docs-layout.md).

```
crates/<name>/docs/
  README.md      # human/engineering overview
  roadmap.md
crates/<name>/adr/
```

- **Agents:** wiki → spec → crate `roadmap` / `adr` — **not** root `README.md` / `CONTRIBUTING.md` for Rule 0
- **Humans:** `CONTRIBUTING.md` · `docs/contributing/`
- **Crate work** → that crate’s `docs/roadmap.md` + `adr/`
- **Repo-wide policy** → `docs/adr`
- **Platform order + index** → `docs/roadmap.md`
- **Wiki** → short pointers; agent ops only under `docs/ai/`
