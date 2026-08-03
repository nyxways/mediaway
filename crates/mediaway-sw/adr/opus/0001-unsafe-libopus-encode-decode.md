# ADR-0001: Opus encode + decode via `unsafe-libopus`, isolated in a dedicated crate

- **Status**: Accepted
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-sw-opus`

**2026-07-31 addendum**: `unsafe-libopus = "0.2"` added to `[workspace.dependencies]` and
this crate's `Cargo.toml`. `cargo deny check` (advisories/bans/licenses/sources) run for
real against the resolved graph — clean, no new exceptions needed. The blocking
prerequisite named in § Consequences is satisfied; implementation may proceed.

## Context

`mediaway_common::CodecKind::Opus` already exists (`crates/mediaway-common/src/lib.rs`), but
no encode or decode path anywhere in the workspace can actually produce or consume Opus
today:

- **Windows**: `mediaway-decoder-windows/src/wmf/opus.rs` wraps the inbox `CMSOpusDecMFT`
  decoder MFT — real, hardware-verified, but **not wired into any public trait** (no
  `AudioDecoder` trait exists in `mediaway-decoder` yet; only `VideoDecoder`, per that
  module's own doc comment). There is **no inbox Opus encoder MFT at all** — confirmed by a
  real `MFTEnumEx(MFT_CATEGORY_AUDIO_ENCODER, ..., MFAudioFormat_Opus)` query returning zero
  results (same module's doc comment). Root `README.md`'s OS·CPU table already records this
  as `Opus | 🆗 / ❌` (Windows: decode 🆗, encode ❌).
- **Web / Linux**: root `README.md` records Opus as `🛠️` on both — no wired path either.
- **CPU/SW**: root `README.md`'s CPU/SW table records Opus as `👻` with the note "No pure
  Rust stack targeted — OS/HW codec APIs only." That note reflected the state of the
  ecosystem at the time it was written; this ADR revisits it because a real, permissively
  licensed, pure-Rust Opus implementation now exists on crates.io and was independently
  found in use (real-time VoIP encode/decode with in-band FEC) in a sibling personal
  project, `nyxie_voice` (`E:\51P\Project-Eddy\Native`, not part of this workspace).

So encode has **zero working path on any platform** today, and decode has exactly one
(Windows WMF), unwired. Both are real gaps this ADR can close with one dependency.

### `unsafe-libopus` (crates.io, `DCNick3/unsafe-libopus`)

libopus 1.3.1 transpiled from C to Rust via `c2rust`, then manually de-duplicated to remove
`#[no_mangle] extern "C"` scaffolding — a genuinely pure-Rust build (no `build.rs`, no
system libopus, no autotools/cmake toolchain requirement). Decoder and encoder are tested
against the IETF-published IETF Opus test vectors, cross-checked byte-for-byte against the
C reference implementation (upstream `README`). No inline assembly / CPU-intrinsic paths —
upstream states a real ~20% CPU cost versus the hand-tuned C build on their test machine;
carried into this crate's own honest cost-contract docs (see Decision).

**Crate name is literal, not marketing**: the exported API is the same C-shaped
`opus_encoder_create` / `opus_encode_float` / `opus_decoder_create` / `opus_decode_float` /
`opus_encoder_ctl!` surface as upstream libopus's own headers, operating on raw
`*mut OpusEncoder` / `*mut OpusDecoder` pointers. Confirmed directly against docs.rs: the
core functions (`opus_encoder_create`, `opus_encode_float`, `opus_decoder_create`,
`opus_decode_float`, `opus_encoder_ctl_impl`, …) are **`unsafe fn`** — the compiler, not
just convention, requires an `unsafe` block at every call site. Only `opus_get_version_string`
and `opus_strerror` are safe. Upstream's own words: "currently... a mostly drop-in
replacement for the `audiopus_sys` crate, with safe APIs potentially coming in the future" —
no maintained safe wrapper exists today.

This is confirmed by real usage: `nyxie_voice/src/voice.rs`'s `EncoderHandle` wraps
`*mut OpusEncoder` with manual `Drop` (`opus_encoder_destroy`) and a hand-justified
`unsafe impl Send`, and every `unsafe-libopus` call in that file (`opus_encoder_create`,
`opus_encoder_ctl!`, `opus_encode_float`, plus decoder-side calls in its test module) is
inside an `unsafe { .. }` block. This ADR does not copy that project's code — it independently
designs an equivalent safe wrapper against this workspace's own types and error conventions
(see Decision).

### Dependency review (`docs/conventions/deps-policy.md` checklist)

| Question | Answer |
|----------|--------|
| Need | Real: closes the *only* Opus encode gap in the whole workspace (no OS API anywhere offers Opus encode; WMF only decodes) and gives Web/Linux a working decode path without waiting on their own OS-backend wiring. |
| License | `unsafe-libopus` 0.2.0 (current `max_version` on crates.io; `0.1.3` is `newest_version`/latest non-yanked-adjacent) — **BSD-3-Clause**, confirmed via the crates.io API (`license` field) and the upstream `Cargo.toml`'s `license = "BSD-3-Clause"`. **Already on `deny.toml`'s allow-list** (`BSD-3-Clause` is listed) — no new exception needed. Matches upstream libopus's own BSD-3-Clause license (this is a faithful transpile, not a relicense). |
| Transitive license | Upstream `Cargo.toml` dependencies: `num-traits`, `num-complex` (`bytemuck` feature), `bytemuck`, `arrayref`, `const-chunks`, `ndarray`, `nalgebra`, `itertools`, optional `hex` (behind a non-default `ent-dump` debug feature this ADR would not enable); dev-only `getrandom`, `insta` (not shipped). All are common, permissively-licensed (MIT/Apache-2.0 family) crates with no known copyleft entanglement — **not independently re-verified with a real `cargo deny check` this session**, because the dependency is not yet in the workspace graph and no shell/build tool was available while drafting this ADR (see Consequences). Running `cargo deny check advisories licenses bans sources` + `cargo tree -d unsafe-libopus` is a **blocking prerequisite** before this ADR can move to Accepted. |
| Maintenance | Active-looking (218 commits on GitHub at review time), purpose-built to become a backend for a fork of the `opus` crate (`unsafe-libopus-backend` feature) — a real third-party validation signal, not a toy. Exact last-commit date not independently pinned this session; re-check before Accepted. |
| API stability | 0.x (`0.1.0` → `0.1.1` → `0.1.2` yanked → `0.1.3` → `0.2.0`) — no semver-stability guarantee yet. Same caveat class already accepted elsewhere in this workspace for `rav1e` (`mediaway-sw` ADR-0002) and `cros-libva` (`mediaway-encoder-linux` ADR-0001). |
| Cost | Pure-Rust graph, no C/asm toolchain — matches this workspace's whole reason for wanting an SW tier. Real, upstream-stated ~20% CPU cost vs. the C reference (no inline asm/SIMD intrinsics) — must be carried into this crate's own rustdoc as an honest cost note ([`caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)), not silently absorbed into "it's just Opus." Compile-time/binary-size impact of `ndarray`/`nalgebra` not measured this session — check at implementation time. |
| Unsafe surface | **The central design question of this ADR** — see Decision. Every call into `unsafe-libopus`'s create/encode/decode/destroy/ctl functions requires `unsafe` at the call site by the compiler's own enforcement (`unsafe fn`), not merely "this dependency happens to contain unsafe code internally" (contrast `rav1e`, whose public `Context`/`Frame` API is fully safe — see `mediaway-sw` ADR-0002). |
| Alternatives | See Alternatives Considered below. |

## Decision

> **Adopt `unsafe-libopus` for both Opus encode and decode**, wrapped in a **new, dedicated
> crate `mediaway-sw-opus`** — not folded into the existing `mediaway-sw` crate.

### Why a dedicated crate, not `mediaway-sw` itself

`mediaway-sw`'s `src/lib.rs` declares `#![forbid(unsafe_code)]` at the crate root, and its
own ADR-0001 states this explicitly as a standing invariant: *"unlike HW backends, this
crate's whole purpose is the no-`unsafe`, no-C-FFI, no-GPL fallback tier."* Its `rav1e`
dependency (ADR-0002) never breaks that invariant, because `rav1e`'s own public
`Config`/`Context`/`Frame` surface is safe Rust — any `unsafe` `rav1e` uses internally stays
"outside this crate's boundary" (ADR-0002's own words).

`unsafe-libopus` is different in kind, not degree: its *public* API is C-shaped and
`unsafe fn` at the boundary. Wrapping it safely **requires** `mediaway-sw`'s own glue code
to contain `unsafe` blocks — something that has never been true of any dependency
`mediaway-sw` has taken so far. Two ways to reconcile that:

1. Downgrade `mediaway-sw`'s crate-level lint from `forbid` to `deny` (the workspace
   default) so one module can locally `#![allow(unsafe_code)]`.
2. Keep `mediaway-sw` exactly as-is (a real, currently-true, testable claim: "every byte of
   this crate's own code is unsafe-free, full stop") and put the one genuinely
   FFI-shaped dependency in its own crate.

This ADR picks **(2)**. `mediaway-sw`'s `forbid` is a load-bearing, user-visible claim (its
own module doc: *"No C codec FFI... Codec logic lives in Rust cores... behind sans-io
adapters"*) that downstream consumers can literally check with `grep unsafe` and get zero
hits — weakening that for one dependency, when a clean crate-boundary split costs one extra
`Cargo.toml` and zero design overhead, is not a good trade. This mirrors why `*-ffi` crates
are split out from cores ([`c-ffi.md`](../../../docs/spec/c-ffi.md), ADR-0004) and why the
workspace lint comment itself calls out **`mediaway-*-sys`**-shaped crates as the intended
home for an `#![allow(unsafe_code)]` override (`Cargo.toml` `[workspace.lints.rust]`
comment) — `unsafe-libopus`'s raw pointer / create-destroy-ctl shape is functionally a
`-sys` crate (C ABI conventions, manual lifetime management) even though it is not literally
`extern "C"`. `mediaway-sw-opus` plays that same "one FFI-shaped boundary, isolated" role;
`mediaway-common`'s own `#![forbid(unsafe_code)]` and every other `mediaway-sw` module stay
untouched.

### Crate name: `mediaway-sw-opus`

Follows the existing `mediaway-sw` family rather than `mediaway-opus` (which would read as a
capability facade expecting `mediaway-opus-windows` / `mediaway-opus-web` platform-backend
siblings — this crate has none; Opus here is one self-contained pure-Rust implementation,
not a facade over OS backends). `mediaway-sw-opus` reads unambiguously as "the CPU/SW-tier
Opus implementation," matching the root `README.md`'s CPU/SW table section this crate fills
in. Per [ADR-0012](../../../docs/adr/0012-unprefixed-reusable-cores.md), it is **not**
unprefixed — it hard-depends on `mediaway_common` types (`AudioFrame`, `Packet`,
`StreamInfo`) from the start, so it is product-bound (`mediaway-*`), not a freestanding
domain core.

### Public low-level surface (design only — no implementation this ADR)

Two RAII session types, method-shaped to match this workspace's existing push/poll
conventions exactly (same staging pattern as `mediaway-sw`'s own `pcm.rs`/`av1.rs`: match
the trait shape now, implement the trait later once a factory needs it):

```text
OpusEncoder   — mirrors mediaway_encoder::AudioEncoder
  open(&OpusEncoderConfig) -> Result<Self, OpusError>
  stream_info(&self) -> &StreamInfo
  push_frame(&mut self, &AudioFrame) -> Result<(), OpusError>
  poll_packet(&mut self) -> Result<Option<Packet>, OpusError>
  flush(&mut self) -> Result<(), OpusError>

OpusDecoder   — mirrors the push/poll shape mediaway-decoder-windows's WmfOpusDecoder
                already uses (no AudioDecoder trait exists in mediaway-decoder yet —
                same honest gap that module's own doc comment names; this ADR does not
                invent that trait either)
  open(&OpusDecoderConfig) -> Result<Self, OpusError>
  stream_info(&self) -> &StreamInfo
  push_packet(&mut self, &Packet) -> Result<(), OpusError>
  poll_frame(&mut self) -> Result<Option<AudioFrame>, OpusError>
  flush(&mut self) -> Result<(), OpusError>
```

Both own a raw `*mut OpusEncoder` / `*mut OpusDecoder` **privately** — the raw pointer never
appears in this crate's public API, matching this workspace's rule that C-shaped surfaces
stay behind a safe boundary and low-level *safe* types (not raw C ones) are what other
Mediaway crates depend on ([`api-layers.md`](../../../docs/spec/api-layers.md)). `Drop`
calls `opus_encoder_destroy` / `opus_decoder_destroy`. `unsafe impl Send` (not `Sync`) is
justified the same way `nyxie_voice::EncoderHandle` justifies it: upstream libopus's own
documented contract is "usable from any single thread, never concurrently from two" — no
thread-affinity, but no free concurrent access either.

`OpusEncoderConfig` carries `sample_rate` / `channels` / `application` (VoIP / Audio /
restricted low-delay — upstream `OPUS_APPLICATION_*`) / `time_base` / optional
`bitrate_bps` / in-band FEC + expected-packet-loss-percent (both real fields in the sibling
project's real usage, and directly load-bearing for real-time voice quality — worth exposing
rather than hardcoding). `open()` defers rate/channel validation to `unsafe-libopus`'s own
`opus_encoder_create` return code (mirrors `rav1e` ADR-0002's "the dependency's own validator
is the source of truth" stance) rather than duplicating Opus's exact legal-rate table locally.

`OpusError` is a `#[non_exhaustive]` `thiserror` enum (per
[`error-handling.md`](../../../docs/conventions/error-handling.md), ADR-0010) carrying the
raw libopus error code and rendering it via the crate's own safe `opus_strerror` for a human
message — not a bare `i32`.

### Documented costly path (per `caveats-and-clarity.md`)

`unsafe-libopus`'s raw functions read/write through `*const f32` / `*mut u8` buffers, not
`Bytes` — `push_frame` / `poll_packet` / `push_packet` / `poll_frame` each need one payload
copy across that raw boundary (PCM in, compressed bytes out, and back). This is not
Zero-Copy and must not be described as such — same honesty bar `mediaway-sw`'s AV1 adapter
already applies to its own unavoidable `rav1e` plane copy. The upstream ~20% CPU cost versus
the hand-tuned C reference (no inline asm/SIMD intrinsics in the transpile) is a second,
separate cost that belongs in this crate's own module doc, not just this ADR.

### Frame-size contract

Opus requires PCM input in one of its fixed legal frame durations (2.5/5/10/20/40/60 ms at
the session's sample rate) — unlike `mediaway-sw::pcm`'s frame-agnostic passthrough.
`push_frame` validates `frame.data.len()` against the configured frame size and rejects
mismatches with a dedicated `OpusError` variant; no internal re-buffering/re-chunking in
this design (callers chunk PCM to the configured frame size themselves, same as
`nyxie_voice`'s own worker loop does). `push_frame` only accepts
`SampleFormat::F32` (`opus_encode_float`) — matches the WMF Opus decoder's own `F32` output,
giving symmetric format handling across this crate and the existing WMF path.
`S16`/`S32` are `OpusError::UnsupportedSampleFormat` in this design (a v1 scope cut,
revisitable later — same pattern as the AV1 adapter accepting only `PixelFormat::I420`).

### Staging: both directions authorized, encode implemented first

Both encode and decode are in scope for this crate (unlike `mediaway-sw`'s H.264, which
staged decode-first because *decode was more tractable to build correctly from scratch*).
That reasoning does not apply here — both directions already come from the same
already-built, already IETF-vector-tested third-party implementation, so tractability is a
wash. Urgency differs instead: **encode has no alternative anywhere in this workspace**
(Windows has zero Opus encode path, full stop), while decode already has an unwired-but-real
WMF fallback on Windows. This ADR recommends implementing `OpusEncoder` first for that
reason, with `OpusDecoder` following in the same crate without needing a second ADR.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `audiopus_sys` (bindgen + system libopus via CMake) + `audiopus`/`opus` crate on top | Exactly the system-library / autotools-cmake build-time dependency this workspace's whole CPU/SW tier exists to avoid (confirmed via real build artifacts observed in the sibling project's own `target/` — a CMake-built `libopus` tree). libopus itself is BSD, but the *build story* is the problem, not the license. |
| Wait for `unsafe-libopus` upstream to ship a safe wrapper | No maintained safe wrapper exists today (upstream's own words: "safe APIs potentially coming in the future") — blocks closing a real, current gap (zero Opus encode anywhere in this workspace) on an unscheduled upstream roadmap item. |
| Hand-roll a pure-Rust Opus encoder/decoder from scratch (mirrors `mediaway-sw`'s H.264 approach) | Opus (RFC 6716) is a hybrid SILK+CELT codec — a multi-year effort, not a from-scratch session slice. `unsafe-libopus` already *is* the permissively-licensed, spec-conformant (IETF test vectors) pure-Rust implementation; this ADR's job is the safe adapter, not reimplementing Opus. |
| Extend `mediaway-sw` directly, downgrading its crate-level lint `forbid` → `deny` | Breaks a real, currently-true, user-visible invariant ("this crate's own code is unsafe-free, full stop," stated in `mediaway-sw` ADR-0001) for every future contributor to that crate, not just this one dependency — see Decision. |
| Name the new crate `mediaway-opus` (facade-style) | Implies OS-backend siblings (`mediaway-opus-windows`, …) this design has none of; `mediaway-sw-opus` matches the existing CPU/SW-tier naming lineage and root `README.md`'s table section. |
| Decode-only or encode-only this ADR (matching `mediaway-sw`'s staged H.264/AV1 precedent) | Both directions ship from the same reviewed dependency at the same quality bar (IETF vectors both ways) — no tractability reason to split the *design* into two ADRs, only the *implementation* order (encode first — see Decision). |

## Consequences

### Positive

- Closes the only Opus encode gap in the entire workspace (currently `❌` on every platform)
  with a real, spec-conformant, permissively-licensed, pure-Rust implementation — no system
  libopus, no CMake/autotools, no new C toolchain requirement.
- Gives Opus decode a platform-independent path (Web/Linux currently have none wired;
  Windows has one real-but-unwired WMF path) without waiting on `AudioDecoder`-trait design
  work in `mediaway-decoder`.
- Keeps `mediaway-sw`'s `#![forbid(unsafe_code)]` invariant intact and testable — no
  weakening of an existing, stated guarantee for the rest of that crate's contributors.
- The unsafe boundary this design draws (raw pointer confined to one crate, RAII + `Drop` +
  documented `Send`, safe push/poll types as the only public surface) mirrors this
  workspace's own established `*-sys`/platform-backend carve-out pattern
  (`#![allow(unsafe_code)]` + `// SAFETY:`) rather than inventing a new one.

### Negative / Trade-offs

- **New dependency, unverified `cargo deny check` this session.** No shell/build tool was
  available while drafting this ADR, so `unsafe-libopus`'s transitive license graph
  (`num-traits`, `num-complex`, `bytemuck`, `arrayref`, `const-chunks`, `ndarray`,
  `nalgebra`, `itertools`) has not been run through `cargo deny check
  advisories licenses bans sources` for real. This is a **blocking prerequisite** for
  Accepted status, not assumed clean.
- Real, upstream-acknowledged ~20% CPU cost vs. the C reference (no inline asm/SIMD) — must
  be documented on the wrapper's own rustdoc, not just here, per
  [`caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md).
- 0.x dependency — no semver-stability guarantee; same caveat class already accepted for
  `rav1e`/`cros-libva` elsewhere in this workspace, revisit on minor bumps.
- One more crate in the workspace graph (`mediaway-sw-opus`) rather than a `mediaway-sw`
  module — a small but real maintenance surface-area cost, accepted here specifically to
  keep `mediaway-sw`'s own unsafe-free claim true.
- Only `SampleFormat::F32` input is accepted in this design; `S16`/`S32` need a follow-up
  conversion layer if a caller needs them.
- Frame-size mismatch is a hard reject, not internal re-buffering — callers must chunk PCM
  to the configured Opus frame size themselves.

## References

- [`docs/spec/vision.md`](../../../docs/spec/vision.md) — license/dependency boundary
- [`docs/spec/sans-io.md`](../../../docs/spec/sans-io.md) — encoder/decoder sessions are
  platform-adapter-shaped, not sans-io cores (this crate follows the same pattern
  `mediaway-sw`'s H.264/AV1/PCM modules already use)
- [`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md) — safe wrapper types as the
  public low-level surface, not the raw C-shaped API
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — costly
  copy + ~20% CPU cost must be documented on the wrapper itself
- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md) — dependency
  review checklist followed above
- [`docs/conventions/error-handling.md`](../../../docs/conventions/error-handling.md) —
  `thiserror`, `#[non_exhaustive]`
- [`docs/adr/0012-unprefixed-reusable-cores.md`](../../../docs/adr/0012-unprefixed-reusable-cores.md) —
  naming rationale for staying `mediaway-*`
- `mediaway-sw/adr/0001-h264-baseline-decoder-first.md` — the `#![forbid(unsafe_code)]`
  invariant this ADR deliberately does not weaken
- `mediaway-sw/adr/0002-rav1e-av1-encode.md` — dependency-review format mirrored above;
  contrast case where the dependency's own public API is safe
- [`mediaway-decoder-windows/src/wmf/opus.rs`](../../mediaway-decoder-windows/src/wmf/opus.rs) —
  existing real (unwired) Opus decode path this crate complements, and the source of the
  "no `AudioDecoder` trait yet" fact cited above
- [`mediaway-encoder/src/audio.rs`](../../mediaway-encoder/src/audio.rs) — `AudioEncoder`
  trait shape mirrored by `OpusEncoder`'s method names
- [`unsafe-libopus` on crates.io](https://crates.io/crates/unsafe-libopus) ·
  [GitHub](https://github.com/DCNick3/unsafe-libopus) (BSD-3-Clause) ·
  [docs.rs](https://docs.rs/unsafe-libopus/latest/unsafe_libopus/)
- IETF RFC 6716 (Opus codec) — see
  [`docs/conventions/external-standards.md`](../../../docs/conventions/external-standards.md)
  for citation policy (not reproduced here)
