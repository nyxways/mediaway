# `tools/scripts/`

Light maintainers’ utilities — **Bun + TypeScript**.

Policy: [`docs/conventions/scripts.md`](../../docs/conventions/scripts.md).

```bash
cd tools/scripts && bun install
bun run fetch-standard.ts --ai-agent <id>    # agents: disclose AI in THIS tool's User-Agent
bun run fetch-standard.ts <id>               # humans (default UA)
bun run fetch-standard.ts verify <id>
bun run fetch-standard.ts pin <id>

bun run fetch-fate-samples.ts --ai-agent     # agents: FATE subset → local/.cache/fate/
bun run fetch-fate-samples.ts                # humans
bun run fetch-fate-samples.ts --force        # re-download

bun run incremental-test.ts                  # cargo-impact → nextest (dev loop)
bun run incremental-test.ts --since main
bun run incremental-bench.ts                 # benches for changed crates (+ reverse-deps)
bun run incremental-bench.ts --since main --no-deps
```

Do **not** reuse `Mediaway-standards-fetch` / `Mediaway-fate-fetch` User-Agents outside their respective scripts.

Incremental test/bench: [`testing.md`](../../docs/conventions/testing.md) · [`benchmarking.md`](../../docs/conventions/benchmarking.md) (dev only — not CI gates).  
Standards policy: [`docs/conventions/external-standards.md`](../../docs/conventions/external-standards.md).
