# External standards

Canonical: [`docs/conventions/external-standards.md`](../../../conventions/external-standards.md) · [`docs/standards/registry.toml`](../../../standards/registry.toml).

- Repo: URL + **BLAKE3** + short Mediaway notes
- Full text: `local/standards/<id>/` (gitignored)
- `bun tools/scripts/fetch-standard.ts [--ai-agent] …` — agents use `--ai-agent` **on this tool only**
- Do not put Mediaway UA on other HTTP clients
