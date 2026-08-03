# Testing

Mediaway uses **tiered placement** and **sibling unit tests**. English only.

## Tiers

| Tier | Purpose | Location | Default gate |
|------|---------|----------|--------------|
| 1. Unit | One function / type behavior | Sibling `<basename>_tests.rs` ([code-style](code-style.md)) | pre-push |
| 2. Integration | Crate public API | `crates/<name>/tests/<scenario>.rs` | pre-push |
| 3. Spec / conformance | Format or cross-impl contract (ISOBMFF, …) | `tests/conformance_*` or crate `tests/conformance_*.rs` | pre-push when present |
| 4. Property | Deterministic pure surfaces | Inside unit/integration via `proptest` | pre-push when present |
| 5. Fuzz | Untrusted byte / packet surfaces | `fuzz/` per crate (cargo-fuzz) | manual / optional CI |
| 6. Bench | Measured hot paths | `benches/` + [benchmarking.md](benchmarking.md) | manual / optional CI |
| 7. Oracle | Compare to PATH `ffmpeg`/`ffprobe` | Integration or dedicated `*_oracle.rs` | **optional** — skip/`#[ignore]` if missing |
| 8. Browser E2E | WASM mux / WebCodecs / capture in Chromium | `tools/e2e-web/tests/*.spec.ts` | **optional** — skip when Playwright/WASM pkg missing |

Time budgets (guidance): unit ≈ ms; integration suite &lt; ~30s; property &lt; ~10s; fuzz/bench/oracle as documented per job; browser E2E &lt; ~2m.

## Tier 1 — Unit

```rust
// src/foo.rs
#[cfg(test)]
#[path = "foo_tests.rs"]
mod tests;
```

```rust
// src/foo_tests.rs
#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn annex_b_converts_when_start_codes_present() { /* … */ }
```

**Rules**

- Inline `#[cfg(test)] mod tests { … }` is **forbidden** (no exceptions for new code).
- One behavior per test; names prefer `behavior_when_condition`.
- Prefer real types over mocks when cheap.
- Root `lib.rs` units live in `src/lib_tests.rs` with `#[path = "lib_tests.rs"]`.
- Exception: `tests/unit_*.rs` only when a **file-level** inner attribute is required and cannot attach to a sibling module (rare).

## Tier 2 — Integration

```
crates/iso-bmff/
├── src/
└── tests/
    └── roundtrip.rs
```

- Use **public** APIs only (`use iso_bmff::…`).
- No shared mutable process state across tests (nextest isolates processes when used).
- Scenario-oriented file names (`roundtrip.rs`, `fmp4_av.cc` → `fmp4_av.rs`).

## Tier 3 — Spec / conformance

When asserting against an external standard or multi-impl contract (e.g. ISOBMFF box layout vs oracle):

- Prefer dedicated `conformance_*.rs` (or a future `tests/conformance/` workspace member).
- Cite registry / ADR; do not paste paywalled standard text ([external-standards.md](external-standards.md)).
- **In-tree:** `iso-bmff` → `tests/conformance_isobmff.rs` (ftyp/moov+mvex/moof+mdat structure from Mediaway crib notes). Always on in the default suite.

## Tier 4 — Property

- Tool: `proptest` in **`[dev-dependencies]` only** ([deps-policy.md](deps-policy.md)).
- Good targets: round-trips, pure bitstream transforms, invariants.
- Do not add `proptest` until a concrete property test lands.

## Tier 5 — Fuzz

- `cargo-fuzz` targets under `fuzz/` when parsers accept untrusted bytes.
- Not required for every crate; add when a parse surface is attack-relevant.
- Related: synthetic demux exception cases in `tests/demux_exceptions.rs` (always on).

## Tier 6 — Bench

- `benches/` + honesty rules in [benchmarking.md](benchmarking.md).
- `criterion` (or equivalent) stays **dev/bench-only**.

## Tier 7 — Oracle (FFmpeg) + FATE corpus

See [§ FFmpeg as test/dev oracle](#ffmpeg-as-testdev-oracle-encouraged). Oracle tests must not break the default suite when binaries are absent.

### FATE samples (demux exception automation)

Optional corpus from the **FFmpeg FATE suite** (third-party samples; not committed). Every container crate with a demuxer carries its own manifest + test — `iso-bmff` (MP4), `ebml-webm` (WebM/Matroska), `riff-wave-core` (WAV), `adts-core` (raw AAC), `mpeg-audio` (MP3/Layer III), `ogg` (Vorbis/Opus transport), `flv-core`, `mpeg-ts-core`:

| Item | Value |
|------|--------|
| Manifest | `crates/<crate>/tests/fate_manifest.txt` (`path` + `oracle_compare` \| `must_not_panic`) |
| Test | `crates/<crate>/tests/demux_exceptions.rs` → `demux_fate_manifest_samples` |
| Env | `MEDIAWAY_FATE_SAMPLES` or `FATE_SAMPLES` = fate-suite **root** (**absolute path** — `cargo test -p <crate>` runs with cwd at the crate root, so a relative path silently resolves to a missing directory and FATE gets skipped) |
| Fetch subset | `bun tools/scripts/fetch-fate-samples.ts` → `local/.cache/fate/` (HTTP from `fate-suite.ffmpeg.org`, scans **every** crate's manifest) |
| Full suite | FFmpeg tree: `make fate-rsync SAMPLES=…` then point the env at that directory |

**Policy:** do not download the entire multi‑GB archive casually (see samples.ffmpeg.org README). Only listed paths via the Bun script (or a local `make fate-rsync` tree). Default `cargo test` **skips** FATE when the env is unset. With the env set:

- All present manifest files: demux must **not panic**
- `oracle_compare` rows + `ffprobe` on PATH: Mediaway's demux counts must match ffprobe. The exact comparison is format-shaped, not identical across crates — packet/frame/tag/access-unit counts (`nb_read_packets` preferred, else `nb_frames`) for packetized formats (`iso-bmff`, `ebml-webm`, `adts-core`, `mpeg-audio`, `mpeg-ts-core`; `flv-core` filters to Audio/Video tags, excluding `ScriptData`, to match ffprobe's semantics), or format fields (`channels`/`sample_rate`) for `riff-wave-core`, which isn't packetized. `ogg`'s raw packet count includes Vorbis/Opus codec header packets that ffprobe's frame count doesn't — reconcile empirically per sample rather than assuming parity; `must_not_panic` is the honest fallback when it doesn't cleanly line up (see `crates/ogg/tests/fate_manifest.txt`). MPEG-TS also needs `ffprobe`'s CSV output de-duplicated (`-count_packets` prints every stream twice for `mpegts` — once program-grouped, once flat — confirmed via `-of json`; a real ffprobe quirk, not a Mediaway bug).
- A manifest row that doesn't fit a crate's documented scope (see that crate's `docs/roadmap.md`) is `must_not_panic`, not silently dropped — the comment on the row says why.
- Multi-`elst` MP4 expands packets above raw `stbl` count via `edts`/`elst`. Edit-list out-of-window samples carry `is_discard` and may have negative PTS/DTS (`mov_neg_first_pts_discard`).
- Encrypted FATE rows: tests supply the documented ClearKey (`1234…9012` hex) via `Demuxer::set_decryption_key` (ADR-0011).

```bash
bun tools/scripts/fetch-fate-samples.ts          # humans
bun tools/scripts/fetch-fate-samples.ts --ai-agent   # agents (required UA flag)
# optional: --force to re-download
export MEDIAWAY_FATE_SAMPLES="$(pwd)/local/.cache/fate"   # must be absolute
cargo test -p iso-bmff --test demux_exceptions
cargo test -p ebml-webm --test demux_exceptions
# ...one per crate listed above
```

User-Agent is `Mediaway-fate-fetch` for **this script only** (not `Mediaway-standards-fetch`). Never commit FATE binaries. Cache under `local/.cache/` (gitignored).

## Writing rules (all tiers)

| Do | Don't |
|----|--------|
| English names and assert messages | Non-English test prose |
| Assert observable behavior | Test private implementation trivia via `pub(crate)` hacks |
| Keep default suite hermetic (no network, no required FFmpeg) | Fail CI solely because `ffmpeg` is missing |
| File-level `unwrap`/`expect` allows on `*_tests.rs` / `tests/*.rs` | Scatter `#[allow]` on production modules for tests |
| Prefer `cargo nextest run` when installed | Rely on flaky shared global state |

## Runners

```bash
cargo nextest run --workspace   # preferred when cargo-nextest is installed
cargo test --workspace          # fallback (hooks already prefer nextest)
cargo test --workspace --doc    # doctests (nextest does not run these)
```

Focused: `cargo nextest run -p iso-bmff`. Config: [`.config/nextest.toml`](../../.config/nextest.toml) (`retries = 0` — fix flakes, don’t hide them).

Pre-push: [`tools/hooks/cargo-prepush.sh`](../../tools/hooks/cargo-prepush.sh).

## Incremental (dev)

Selective runs for the **local edit loop** via `cargo-impact` + nextest (Bun wrapper).

| Tool | Command |
|------|---------|
| Incremental tests | `cd tools/scripts && bun run incremental-test.ts` |
| vs `main` | `bun run incremental-test.ts --since main` |
| Extra nextest args | `bun run incremental-test.ts -- --no-fail-fast` |

Requires: `cargo install cargo-impact cargo-nextest`. Default `--confidence-min 0.5`.

**Not a gate:** do **not** put these in pre-push or PR CI. Full `cargo nextest` / `cargo test --workspace` remains the merge gate (impact can miss tests). Empty impact filter → exit 0 (nothing to run).

Incremental benches: [benchmarking.md](benchmarking.md) § Incremental (dev).

## Test media fixtures (absolute)

| Rule | Detail |
|------|--------|
| Generate in Rust | Fixtures come from `mediaway-test-media` (or crate-local generators that call it) |
| Local cache only | Written under `local/.cache/test-media/` (gitignored) |
| **Hash verify** | Cache hit only counts if **BLAKE3** matches the expected digest; stale/corrupt files are regenerated |
| Never commit binaries | No checked-in media/raw blobs for tests |
| Deterministic | Same generator inputs → same bytes (and thus same digest) |
| Canonical mint ≠ FFmpeg | Generators stay permissive / Pure Rust (or Mediaway encoders once available) |

Optional override: `MEDIAWAY_TEST_MEDIA_CACHE` = absolute path to the cache root.

### Workflow

```text
test needs fixture + expected BLAKE3
        │
        ▼
mediaway_test_media::ensure(name, expected_hex, generate)
        │
        ├─ file exists AND blake3(file) == expected  → return PathBuf
        ├─ missing OR hash mismatch                  → regenerate
        └─ after generate: blake3 must == expected   → else HashMismatch error
```

Commit **generator source** and the **expected hex constant** (in Rust), not the media bytes.

### Hook

`tools/hooks/forbid-test-media.sh` (pre-commit) blocks staging of common media extensions.

## FFmpeg as test/dev oracle (encouraged)

**Product graph stays FFmpeg-free.** System `ffmpeg` / `ffprobe` may be used **aggressively** as an optional reference in tests and local development. ADR: [`docs/adr/0002-system-oracle.md`](../adr/0002-system-oracle.md).

| Do | Don't |
|----|--------|
| Call `ffmpeg` / `ffprobe` on `PATH` via `Command` | Add FFmpeg crates or link `libav*` |
| Skip / `#[ignore]` when binary missing | Fail the default suite solely because FFmpeg is absent |
| Compare Mediaway output to FFmpeg output | Require FFmpeg at library or shipped-CLI runtime |
| Optional CI job with FFmpeg installed | Redistribute FFmpeg with Mediaway releases |
| Use as oracle / probe / golden check | Use FFmpeg to mint canonical `mediaway-test-media` fixtures |
| Side-by-side **perf** on the same `machine_id` ([benchmarking.md](benchmarking.md)) | Link or redistribute FFmpeg with Mediaway |

Helper pattern: detect once (`which` / `Command::new("ffprobe").arg("-version")`), then `return` early or mark ignored so `cargo test --workspace` stays green on a clean machine.

## Tier 8 — Browser E2E (Playwright)

| | |
|--|--|
| Location | `tools/e2e-web/tests/*.spec.ts` |
| Build | `cd tools/e2e-web && bun run build:wasm` (wasm32 + `wasm-bindgen` → `pkg/`) |
| Gate | **optional** CI / manual — not pre-push |
| Fake media | Chromium `--use-fake-ui-for-media-stream` + `--use-fake-device-for-media-stream` for headless capture smoke |

See [`tools/e2e-web/README.md`](../../tools/e2e-web/README.md).

## Related

- [code-style.md](code-style.md) — sibling unit test placement · unwrap allows
- [benchmarking.md](benchmarking.md) — Tier 6 honesty
- Wiki: [`docs/ai/wiki/meta/testing.md`](../ai/wiki/meta/testing.md)
