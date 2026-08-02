# Local workspace (not committed)

This directory is a **machine-local** scratch area for humans and coding agents.

| Intended contents | Examples |
|-------------------|----------|
| Personal / agent memory | Session notes, “what I tried on this PC”, open questions |
| Machine-specific facts | GPU model, driver versions, local paths, device IDs |
| Experiments | Throwaway Rust/`main` snippets, probe scripts, one-off captures |
| Local overrides | Env snippets, bench raw outputs (`local/benches/`), Criterion HTML copies |
| External standards | Downloads under `standards/` — see [`external-standards.md`](../docs/conventions/external-standards.md) |

**Never put secrets here that you would not want on disk** — treat like any local folder. Still do not commit: everything except this README (and `.gitignore`) is ignored.

## Suggested layout

Create as needed (all gitignored):

```text
local/
  README.md           ← tracked (this file)
  .gitignore          ← tracked
  agent/              ← agent scratch notes
  machine/            ← hardware notes
  experiments/        ← throwaway code
  benches/            ← raw bench outputs
  standards/          ← fetched docs; BLAKE3 must match docs/standards/registry.toml
    <id>/source.url
    <id>/source.ua      ← User-Agent (human vs ai-agent)
    <id>/<filename>
  tmp/                ← anything ephemeral
```

## Rules

1. **Do not commit** `local/**` content (hooks/reviewers treat accidental staging as a mistake).
2. **Do not** relocate project SSOT here — durable knowledge goes to `docs/ai/wiki/`, specs, ADRs.
3. Agents: prefer writing durable findings to the wiki; use `local/` for **this machine** or **this session** only.
4. **Standards:** wiki/spec hold **URLs + Mediaway notes** only; full documents live under `local/standards/` when needed ([`external-standards.md`](../docs/conventions/external-standards.md)).
5. Experimental crates under `local/experiments/` are fine; they are not workspace members unless you deliberately add them (usually don’t).

## Related

- Cache for generated test media: `local/.cache/` (also gitignored)
- Bench machine profiles: [`docs/benchmarks/machines.md`](../docs/benchmarks/machines.md)
- Agent rules: [`AGENTS.md`](../AGENTS.md)
- Contributor agent brief: [`docs/contributing/for-agents.md`](../docs/contributing/for-agents.md)
