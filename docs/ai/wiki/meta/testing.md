# Testing layout

Canonical: [`docs/conventions/testing.md`](../../../conventions/testing.md).

| Tier | Where |
|------|--------|
| Unit | Sibling `foo_tests.rs` — **no** inline `mod tests {…}` |
| Integration | `crates/*/tests/*.rs` |
| Spec / conformance | `conformance_isobmff.rs` (always) · `conformance_oracle.rs` (ffprobe optional) |
| Demux exceptions | Synthetic always · FATE via `MEDIAWAY_FATE_SAMPLES` + `fate_manifest.txt` (`oracle_compare` / `must_not_panic`) — every container crate (`iso-bmff`, `ebml-webm`, `riff-wave`, `adts`, `mpeg-audio`, `ogg`, `flv`, `mpeg-ts`) |
| Property / fuzz / bench | when justified |
| Browser E2E | `tools/e2e-web` (Playwright; optional CI) |
| Fetch FATE subset | `bun tools/scripts/fetch-fate-samples.ts` [--ai-agent] → `local/.cache/fate/` |
| Incremental (dev) | `bun run incremental-test.ts` / `incremental-bench.ts` — not a CI gate |
| ClearKey CENC | [`crypto`](crypto.md) · `iso-cenc` |

Runner: prefer `cargo nextest run`; doctests via `cargo test --doc`.  
Dev install for incremental tests: `cargo install cargo-impact cargo-nextest`.
Fixtures: [test-media](test-media.md).
