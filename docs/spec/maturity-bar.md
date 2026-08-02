# Maturity bar (nascent stack)

Mediaway is a **greenfield** media stack. Established production tools have decades of battlefield input, edge-case lore, and operational trust. This doc lists what we must **earn** over time — without pretending we already have that history.

**Status today:** early development; not for production ([`status.md`](status.md)).

## What “caught up” means (dimensions)

Breadth of formats/codecs is fine. Maturity means callers can trust Mediaway on **correctness, stability, and performance** for the scopes we claim — feature lists without evidence are not enough. There is **no** mandate to ship “narrow verticals” before wider goals ([ADR-0008](../adr/0008-no-narrow-vertical-mandate.md)).

| Dimension | What to build |
|-----------|----------------|
| **Real-world input** | Corpora of common and *broken* files; round-trips; fuzzers on demux/bitstream; continuous intake of bug-shaped media from users |
| **Reference checks** | Optional **external reference tools** as oracles in tests/dev ([`testing.md`](../conventions/testing.md)) — compare timestamps, hashes, bitstream properties; never required at runtime |
| **HW + Zero-Copy truth** | Platform paths that stay Zero-Copy by default — GPU-resident **or** shared CPU (no payload memcpy); measured regressions when a copy/readback creeps in ([`gpu-interop.md`](gpu-interop.md), [wiki marks](../ai/wiki/zero-copy/marks.md), [`caveats-and-clarity.md`](caveats-and-clarity.md)) |
| **Performance evidence** | Public benchmarks per [benchmarking.md](../conventions/benchmarking.md) (labeled `zc` / `copy` / `readback` / …) on registry [`machine_id`](../benchmarks/machines.md) profiles; **Mediaway + system FFmpeg side-by-side** when comparable; CI smoke + deeper HW runs; no “feels fast” claims without numbers |
| **Determinism & tests** | Generated fixtures + BLAKE3; property tests; soak / stress where sessions and pools leak |
| **Diagnostics** | Actionable errors (not opaque `false`); optional tracing; dump hooks for failing bitstreams (privacy-aware) |
| **Interop surface** | Documented behavior vs common players/editors/browsers for formats we support; known divergences listed honestly |
| **API / ABI discipline** | Semver for Rust; later C ABI versioning ([`c-ffi.md`](c-ffi.md)); deprecation windows; no silent behavior flips |
| **Platform matrix** | Explicit support marks in root [`README.md`](../../README.md#codec-support) (`✅`/`🆗`/`🛠️`/`👻`/…; Windows → Web → Linux → …); CI where feasible |
| **Security** | Fuzz + deny/advisories; untrusted-input assumptions on demux; memory-safety culture (`unsafe` boundaries) |
| **Operability** | Clear caveats, runbooks for HW init failures, driver quirks catalog (wiki/crate docs) |

## Practical workstreams (ongoing)

1. **Corpus & fuzz** — demux/mux/bitstream first; grow with each format.
2. **Oracle CI job (optional)** — reference-tool compares on a labeled fixture set; default `cargo test` stays tool-free.
3. **Perf baselines** — follow [benchmarking.md](../conventions/benchmarking.md) + [machines.md](../benchmarks/machines.md); one suite per major path with recorded numbers and `machine_id`.
4. **Failure catalog** — every painful driver/container bug becomes a regression test + short wiki note.
5. **Compatibility notes** — per format/platform: what we guarantee vs best-effort.
6. **Production gate** — only flip [`status.md`](status.md) when claimed scopes have corpora + benches + soak evidence + a stable API note for that scope.

## What we will not do

- Claim production readiness from scaffolding or a single happy-path demo
- Hide copies/fallbacks to win a benchmark screenshot ([`caveats-and-clarity.md`](caveats-and-clarity.md), [`benchmarking.md`](../conventions/benchmarking.md))
- Trade the permissive license graph for a shortcut ([`vision.md`](vision.md))

## Relationship to vision

**No performance surrender** and **honest by default** are how a new library competes on trust: fewer silent footguns, measurable HW paths, and evidence-backed claims.

Related: [`overview.md`](overview.md) · [`docs/roadmap.md`](../roadmap.md) · testing conventions.
