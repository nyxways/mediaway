# ADR-0002: AV1 encode via `rav1e`, sans-io adapter around its `Context`/`Frame` API

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-sw`

## Context

`mediaway-sw`'s roadmap Stage 2 names "`rav1e` (or in-tree Rust) behind a sans-io adapter
— ADR", left open by [ADR-0001](0001-h264-baseline-decoder-first.md), which explicitly
deferred any new dependency review to this stage. The root README marks AV1 under
`CPU / SW` as 🛠️ (planned) with the note "e.g. `rav1e` (Rust) behind sans-io adapter —
ADR". Per [`docs/ai/wiki/license/policy.md`](../../../docs/ai/wiki/license/policy.md), this
crate's whole purpose is a pure-Rust, no-C-FFI, no-GPL fallback tier — any encoder that
lands here must clear the same bar as the existing H.264 bitstream code: permissive
license, no unsafe in this crate's own code, sans-io.

[`rav1e`](https://github.com/xiph/rav1e) (crates.io `rav1e`) is a real, complete,
production-used AV1 encoder written in pure Rust — used in production by Firefox,
`ffmpeg`'s `librav1e`, and others — not a toy or a wrapper around a C library. It is BSD-2-
Clause licensed. Unlike the H.264 decode work in this crate (a from-scratch bitstream
parser, because no acceptable pure-Rust H.264 decoder crate exists), `rav1e` is itself the
actual encoder implementation; this ADR's job is the *adapter* around it, not reimplementing
AV1 rate control / mode decision / entropy coding.

### `rav1e` review (`docs/conventions/deps-policy.md` checklist)

| Question | Answer |
|----------|--------|
| Need | Real: an AV1 bitstream writer (rate control, mode decision, CDEF/loop-restoration, entropy coding) is a multi-year effort, not a local-code substitute. |
| License | `rav1e` 0.8.1 — **BSD-2-Clause** (confirmed via crates.io API and upstream `LICENSE`), already on `deny.toml`'s allow-list. |
| Transitive license | `cargo deny check licenses` (below) surfaced one issue: `libfuzzer-sys` (an unconditional `[target.'cfg(fuzzing)'.dependencies]` edge of `rav1e`, used only by `cargo fuzz`, never built by Mediaway) declares `(MIT OR Apache-2.0) AND NCSA`. NCSA is OSI-approved/FSF-free permissive, just not previously on the allow-list; added as a **crate-scoped** `[[licenses.exceptions]]` in `deny.toml` rather than widening the global allow-list for one non-shipped edge. No GPL/LGPL/AGPL/SSPL/BUSL anywhere in the graph. |
| Maintenance | Active: xiph.org project, recent releases (0.8.1 current), real production users (Firefox, `ffmpeg`). |
| API stability | 0.x, but the `Config`/`Context`/`Frame`/`Packet` push/poll surface used here has been stable across the 0.7→0.8 line; this adapter only touches that stable surface. |
| Cost | With `default-features = false` (this ADR's choice — see Decision), the dependency graph pulls in no C/asm toolchain, no threadpool crate activation, no CLI/binary deps. Still a non-trivial pure-Rust graph (`av1-grain`, `av-scenechange`, `v_frame`, `arrayvec`, `itertools`, …) — acceptable for a real encoder. |
| Unsafe surface | This crate's own code stays `#![forbid(unsafe_code)]`; `rav1e` and its dependencies carry their own internal `unsafe` (SIMD, etc.), reviewed by upstream, outside this crate's boundary — same posture as depending on any other codec-shaped crate. |
| Alternatives | Hand-rolled pure-Rust AV1 encoder: out of scope for one crate/session, and `rav1e` already *is* the pure-Rust, permissively-licensed option the roadmap named. No other maintained pure-Rust AV1 encoder crate exists on crates.io at review time. |

### `cargo deny check` after adding `rav1e`

```
advisories ok, bans ok, licenses ok, sources ok
```

Two additive `deny.toml` changes were needed to reach a clean check, both scoped narrowly
rather than loosening global policy:

1. `[[licenses.exceptions]]` — `{ allow = ["NCSA"], name = "libfuzzer-sys" }` (the
   `cfg(fuzzing)`-only edge described above).
2. `[advisories].ignore` — `"RUSTSEC-2024-0436"` (`paste`, an unconditional
   compile-time-only proc-macro dependency of `rav1e` 0.8.1, upstream-archived as
   unmaintained; the advisory itself states "no safe upgrade is available" since `rav1e`
   has not migrated to the `pastey` fork yet). Not a vulnerability — no runtime code ships
   from a proc-macro crate. Revisit when `rav1e` drops it.

## Decision

> Depend on **`rav1e` 0.8** (workspace-pinned, minor-locked per `deps-policy.md`) with
> **`default-features = false`** — this disables `rav1e`'s `asm` feature (which pulls in
> `nasm-rs` + `cc`, a C/asm toolchain requirement this ADR explicitly avoids per the task's
> "genuinely implementable without any external hardware or proprietary SDK" bar),
> `binaries` (CLI-only deps: `clap`, `y4m`, …), `threading` (`rayon/threads` — the encoder
> falls back to `maybe-rayon`'s single-threaded shim, which is fine for a correctness-first
> adapter; a later ADR can opt back into `threading` if throughput matters), and
> `git_version`. The result is a genuinely pure-Rust, single-threaded, no-toolchain AV1
> encoder build — real and hardware-independent, matching this crate's whole reason to
> exist.

> Implement [`av1::Av1Encoder`](../src/av1.rs) as a thin sans-io wrapper around
> `rav1e::{Config, Context, EncoderConfig}`:
>
> - `Av1Encoder::open(&Av1EncoderConfig) -> Result<Self, Av1Error>` builds a `rav1e`
>   `EncoderConfig` from the caller's width/height/time_base/bitrate/speed/low_latency,
>   fixes `chroma_sampling = ChromaSampling::Cs420` and `bit_depth = 8` (only 8-bit 4:2:0 is
>   supported — see below), and calls `Config::new().with_encoder_config(enc).new_context()`.
>   `rav1e`'s own `Config::validate()` (invoked internally by `new_context()`) is the single
>   source of truth for encoder-config constraints (e.g. the 16px minimum dimension) —
>   this adapter does not duplicate that validation.
> - `push_frame(&VideoFrame) -> Result<(), Av1Error>` accepts only
>   [`PixelFormat::I420`](../../mediaway-common/src/formats.rs) CPU-resident frames — `rav1e`'s
>   own native `Frame<u8>` layout is already planar 4:2:0, so no packed↔planar or chroma
>   up/down-sampling conversion is needed. The one unavoidable copy (packed `Bytes` into
>   `rav1e`'s own padded/aligned `Frame` plane storage, via `Plane::copy_from_raw_u8`) is
>   documented with a rustdoc "Costly path" section on `push_frame`, per
>   [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — `rav1e::Context::new_frame`
>   always owns its plane buffers, so there is no Zero-Copy handoff possible through the
>   public `rav1e` API; this mirrors how `rav1e`'s own reference CLI
>   (`src/bin/decoder/y4m.rs`) fills frames from decoded input.
> - `poll_packet()` / `flush()` mirror `mediaway_encoder::VideoEncoder`'s push/poll/drain
>   shape, mapping `rav1e::EncoderStatus` variants to a local `Av1Error` — every match is
>   written **exhaustively** (no wildcard arm) so a future `rav1e` status addition is a
>   compile error here, not a silently-swallowed case; variants `rav1e` documents as
>   unreachable from a given call site are still mapped (defensively) rather than triggering
>   a `_ => unreachable!()`/panic path, per the workspace's no-panic-outside-tests rule.
> - `stream_info()` returns `mediaway_common::StreamInfo::Video` with `codec: CodecKind::Av1`
>   and `extra_data` set to `rav1e`'s `container_sequence_header()` output **immediately at
>   `open()`** (it only depends on static encoder config, not on any frame having been
>   pushed), unlike H.264 SPS/PPS which needs real bitstream inspection.

> **No trait impl yet.** Like ADR-0001's H.264 decision, `Av1Encoder` does **not** implement
> `mediaway_encoder::VideoEncoder` in this ADR's implementation — `mediaway-sw` still depends
> on `mediaway-common` only, plus `rav1e`. The method names/signatures
> (`push_frame(&VideoFrame)`, `poll_packet() -> Result<Option<Packet>, _>`, `flush()`) are
> written to match that trait exactly so wiring it in later (once a factory actually needs
> `mediaway-sw` as a fallback) is a mechanical `impl VideoEncoder for Av1Encoder` with no
> reshaping.

> **Packet timestamps are `rav1e`'s own frame ordinal, not the caller's `VideoFrame::pts`.**
> The public `rav1e` `Context`/`Frame` API has no per-frame timestamp hook — `rav1e` tracks
> display order internally via `Packet::input_frameno`. Rather than build an unverified
> pts-passthrough queue (fragile without a real AV1 decoder to check reordering against —
> explicitly out of scope this session, see Consequences), `Packet::pts`/`dts` are set from
> `input_frameno` directly, in the session's configured `time_base`. This is documented on
> `convert_packet` in `src/av1.rs`, not left as a silent surprise.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `rav1e` with default features (`asm`, `threading`, `binaries`, `git_version`) | Pulls in `nasm-rs`/`cc` (a C/asm toolchain requirement — against this crate's "genuinely no external toolchain" bar) and CLI-only deps (`clap`, `y4m`) never used by this adapter. `default-features = false` gets a real, still-functional (if slower/single-threaded) encoder without any of that. |
| Support NV12 input alongside I420 (matching other backends' preferred format) | Would need a packed-semiplanar→planar de-interleave copy in addition to the byte copy that's already unavoidable for `rav1e`'s owned `Frame` storage. I420 is `rav1e`'s *native* layout — supporting only it keeps the adapter's one caveat (the plane copy) instead of adding a second, avoidable one. NV12 can be added later behind its own documented conversion if a caller needs it. |
| Hand-rolled pure-Rust AV1 bitstream encoder | Multi-year effort (rate control, mode decision, CDEF, entropy coding); `rav1e` already is the permissively-licensed pure-Rust option the roadmap named — reimplementing it would contradict "your job is the adapter, not reimplementing AV1." |
| Thread caller `VideoFrame::pts` through to output `Packet::pts` via a FIFO queue keyed to `rav1e`'s internal reordering | `rav1e`'s public API gives no guarantee about the exact 1:1 correspondence between push order and packet-emission order beyond "packets are numbered by the shown frame's input ordinal" — verifying a FIFO-queue mapping is correct under B-frame reordering needs a real AV1 decoder round-trip, which this session explicitly does not have available (see Consequences). Honestly documenting the `input_frameno`-based timestamp instead of guessing was judged safer than a plausible-looking but unverified pts remap. |
| Enable `threading` (`rayon/threads`) for encode throughput | Not needed for this stage's scope (a correctness-first adapter); `default-features = false` keeps the dependency graph and build story simpler. Can be revisited in a follow-up ADR if encode throughput becomes a real requirement. |

## Consequences

### Positive

- A real, hardware-independent, fully pure-Rust AV1 encoder now exists behind a sans-io
  adapter in `mediaway-sw` — genuinely usable today, not aspirational (unlike the H.264
  decode path, which still has no pixel-reconstruction loop per ADR-0001).
- `cargo deny check` (advisories, bans, licenses, sources) passes clean with narrowly-scoped,
  documented exceptions — no GPL/LGPL/AGPL/copyleft anywhere in the graph.
- The method shapes (`push_frame`/`poll_packet`/`flush`, `StreamInfo`) are already
  trait-compatible with `mediaway_encoder::VideoEncoder`, so wiring this in as a real SW
  fallback later is mechanical.
- Verified this session end-to-end: `Av1Encoder::open` with a real config, pushing several
  synthetic gradient frames, flushing, and confirming the output contains a real
  `OBU_SEQUENCE_HEADER` and non-trivial encoded bytes (see Verification below) — not just a
  compiles-but-untested wrapper.

### Negative / Trade-offs

- **No AV1 decoder available to round-trip against.** This session validates *structurally*
  (valid OBU framing, non-empty compressed output, encoder accepts frames and flushes
  cleanly) rather than *semantically* (decoding the output and comparing pixels to the
  input). A pure-Rust/pure-permissive AV1 decoder crate to close this gap does not exist
  today (a real `dav1d` binding would pull in a C library and/or non-permissive-adjacent
  concerns, which this crate's whole purpose forbids); system `ffmpeg`/`ffprobe` as a
  dev-only oracle ([ADR-0002 workspace](../../../docs/adr/0002-system-oracle.md)) could
  close this gap in a follow-up session but was not exercised here.
- `Packet::pts`/`dts` do not carry the caller's original `VideoFrame::pts` through — see the
  Decision section's honest note. A caller relying on exact input timestamp fidelity on
  output packets needs to track that mapping itself (input push order ↔ `input_frameno`)
  until/unless a future revision adds a verified reordering-aware remap.
- Only 8-bit `PixelFormat::I420` input is accepted; NV12/BGRA/RGBA and 10-bit are
  `Av1Error::Unsupported` until a follow-up adds the relevant conversion (and, for 10-bit, a
  `PixelFormat` variant that does not exist in `mediaway-common` yet).
- `rav1e` is 0.x — no semver-stability guarantee; a future minor bump could shift behavior
  and need re-review (same caveat class as `cros-libva` in `mediaway-encoder-linux` ADR-0001).
- Default `default-features = false` build is single-threaded (`maybe-rayon` shim) — slower
  than a `threading`-enabled build. Acceptable for a fallback tier; revisit if throughput
  matters.

## Verification (this session)

- `cargo test -p mediaway-sw` — 53 tests pass, including `av1::tests::encode_a_few_frames_and_flush_produces_valid_looking_av1`:
  pushes 6 synthetic 64×64 I420 gradient frames (generated in-test, no committed media —
  [`docs/conventions/testing.md`](../../../docs/conventions/testing.md)), flushes, drains all
  packets, and asserts (a) at least one packet, (b) total encoded bytes > 64 (non-trivial),
  (c) at least one keyframe packet, and (d) the first packet's AV1 "low overhead bitstream
  format" OBUs (walked via a minimal in-test LEB128/OBU-header scanner) include
  `obu_type == 1` (`OBU_SEQUENCE_HEADER`, AV1 spec § 5.3.2).
- `cargo clippy -p mediaway-sw --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean for every file this ADR's change touched (pre-existing drift
  in `h264/**`, out of this ADR's scope, is untouched and unrelated).
- `cargo deny check` — `advisories ok, bans ok, licenses ok, sources ok`.
- **Not verified**: real hardware is not applicable here (this is a pure-CPU, no-GPU-device
  path by construction — unlike the VA-API ADR's hardware caveat, there is no "hardware" this
  adapter could fail to find), but pixel-level decode-side correctness is unverified per the
  Negative/Trade-offs note above.

## References

- [`rav1e` on crates.io](https://crates.io/crates/rav1e) ·
  [GitHub](https://github.com/xiph/rav1e) (BSD-2-Clause)
- [ADR-0001](0001-h264-baseline-decoder-first.md) — this crate's sans-io boundary and
  no-new-dependency-yet precedent this ADR follows up on
- [`docs/ai/wiki/license/policy.md`](../../../docs/ai/wiki/license/policy.md) ·
  [`sw-scaffold.md`](../../../docs/ai/wiki/license/sw-scaffold.md)
- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md) — dependency
  review checklist followed above
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — costly-path
  rustdoc requirement satisfied on `Av1Encoder::push_frame`
- [`docs/spec/sans-io.md`](../../../docs/spec/sans-io.md)
- `mediaway-encoder-linux` ADR-0001 (`cros-libva` dependency review) — checklist format
  mirrored above
- [`mediaway-encoder/src/video.rs`](../../mediaway-encoder/src/video.rs) — `VideoEncoder`
  trait shape mirrored by `Av1Encoder`'s method names
- Crate roadmap: [`docs/roadmap.md`](../docs/roadmap.md)
- AV1 Bitstream & Decoding Process Specification § 5.3.2 (`open_bitstream_unit`/OBU header) —
  see [`docs/conventions/external-standards.md`](../../../docs/conventions/external-standards.md)
  for citation policy (not reproduced here)
