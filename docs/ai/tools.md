# Tools and verification

## Day-to-day

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# optional
cargo nextest run --workspace
cargo deny check advisories licenses bans sources
```

## Hooks

```bash
cargo install lefthook cargo-deny
lefthook install
lefthook run pre-commit   # manual
```

Details: [`docs/conventions/hooks.md`](../conventions/hooks.md)

## Dev scripts (Bun + TypeScript)

Light maintainers’ utilities: [`docs/conventions/scripts.md`](../conventions/scripts.md) · `tools/scripts/`.

```bash
bun --version          # when a task needs tools/scripts
# cd tools/scripts && bun install && bun run <script>
```

Do not add Node/Python script trees for shared tooling. Lefthook gates stay bash.

External standards: fetch into `local/standards/` and verify **BLAKE3** against [`docs/standards/registry.toml`](../standards/registry.toml) — [`docs/conventions/external-standards.md`](../conventions/external-standards.md).

```bash
cd tools/scripts && bun install
bun run fetch-standard.ts verify <id>
```

## License gate

When adding deps or suspecting FFmpeg-family leakage:

```bash
cargo deny check advisories licenses bans
cargo tree -i ffmpeg-next   # must not resolve
```

**New crates:** follow [`docs/conventions/deps-policy.md`](../conventions/deps-policy.md) (need, transitive license, maintenance, cost, alternatives) and justify in the PR before merge.

System `ffmpeg`/`ffprobe` for oracle tests: [`docs/conventions/testing.md`](../conventions/testing.md) · ADR-0002.

Policy: [`docs/conventions/security.md`](../conventions/security.md) · [`deps-policy.md`](../conventions/deps-policy.md)

## Test media cache

```bash
# default: <repo>/local/.cache/test-media/
# optional override:
# export MEDIAWAY_TEST_MEDIA_CACHE=/path/to/cache
cargo test -p mediaway-test-media
```

Do not commit media binaries — [`docs/conventions/testing.md`](../conventions/testing.md).

## Benchmarks

```bash
cargo bench -p <crate>   # once benches exist
```

Rules: [`docs/conventions/benchmarking.md`](../conventions/benchmarking.md) — label `zc`/`copy`/`readback`; **`machine_id`**; **`oracle_ref`** beside Mediaway when comparable; default CI stays light.

## FFmpeg oracle (optional)

```bash
ffmpeg -version    # or ffprobe -version
# tests that need it should skip if missing
```

## Wiki size

`docs/ai/wiki/**/*.md` — **≤100 lines per file**. Claude PreToolUse hook (`check-wiki-size.sh`) rejects Write/Edit over the limit.
If over: split pages and update the category `index.md`.

Wiki and all docs: **English** (`AGENTS.md` § Language policy).

## ADR / crate scaffold

Claude slash commands: `/adr`, `/scaffold-crate`, `/issue` (`.claude/commands/`)
