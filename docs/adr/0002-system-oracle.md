# ADR-0002: System CLI as optional test/dev oracle only

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Mediaway's product identity is **FFmpeg-less**: no FFmpeg in the shipped dependency graph, no GPL/LGPL codecs in library crates (`deny.toml` enforced).

Integration tests still benefit from a widely available reference tool on `PATH` for decode/mux round-trips, bitstream checks, and golden compares — without improving shipped license story by banning external CLI use entirely.

## Decision

> **Encourage** system-installed reference CLIs (`ffmpeg` / `ffprobe` on `PATH`) as an **optional oracle** in tests and local dev. **Forbid** linking or vendoring FFmpeg (or FFmpeg Rust bindings) into Mediaway crates or published binaries.

- **Allowed:** `Command` invocation when binary on `PATH`; skip/`#[ignore]` when missing (default `cargo test` passes without oracle); optional CI oracle jobs; side-by-side perf compare (`oracle_ref` per [`benchmarking.md`](../conventions/benchmarking.md)); not as product encode/decode path
- **Forbidden:** Cargo deps on `ffmpeg-next`/`ffmpeg-sys*`/`ac-ffmpeg`/etc.; redistributing FFmpeg; runtime oracle requirement; external CLI as canonical test-media mint; product names containing `ffmpeg`/`ffprobe` (use `mediaway-avcli` / `mediaway-avprobe`)
- **Scope:** Dev/CI only; does not change MIT OR Apache-2.0 product license

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Never run external oracle | Slower coverage; no license gain for shipped crates |
| Link FFmpeg via crates for tests | Contaminates graph; easy to leak into non-test builds |
| Vendor FFmpeg in-repo | License redistribution risk |

## Consequences

- Strong reference comparisons without product license risk; oracle tests environment-dependent (must skip cleanly)

## References

- [`docs/conventions/testing.md`](../conventions/testing.md), [`benchmarking.md`](../conventions/benchmarking.md), [`deps-policy.md`](../conventions/deps-policy.md)
- `deny.toml` `[bans]`
