# mediaway-ffi — auto-encode → fMP4 C ABI

Second `mediaway-*-ffi` crate, after `mediaway-ffi`. Wraps
`mediaway::platform::AutoEncoder` + `EncodeSession` (auto OS/GPU
video encoder wired straight into a single-track fMP4 muxer) over a C ABI.
Full design:
[`crates/mediaway-ffi/adr/0001-auto-encode-c-abi.md`](../../../../crates/mediaway-ffi/adr/0001-auto-encode-c-abi.md),
GPU input:
[`adr/0002-gpu-frame-input-c-abi.md`](../../../../crates/mediaway-ffi/adr/0002-gpu-frame-input-c-abi.md).

## Shape

- Two opaque handles: `AutoEncoderHandle = Box<dyn VideoEncoder>` (no
  `poisoned` flag — its only ops destroy the pointer unconditionally) and
  `EncodeSessionHandle { poisoned, inner: EncodeSession<Box<dyn VideoEncoder>> }`.
  `mediaway_encode_session_open` **unconditionally consumes** the encoder
  handle, success or failure (matches `EncodeSession::open(encoder: E)`
  taking `E` by value) — do not close the encoder handle afterward.
- `mediaway_auto_video_encode_config_t` is a plain value struct (no handle,
  no free) — fields: `codec`, `width`, `height`, `time_base`, `bitrate_bps`,
  `pixel_format`, `gpu_device` (ADR-0002; `MEDIAWAY_GPU_DEVICE_NONE` keeps the
  CPU-only path). **Still deferred:** `backend`, `max_path_class` —
  `AutoVideoEncodeConfig`'s own `max_path_class` default (`CpuUpload`) already
  permits Zero-Copy/GPU-copy, so `gpu_device` alone is enough to reach the GPU
  path from C.
- `mediaway_video_frame_t` (write_frame input) carries `storage_kind`
  (`CPU`/`GPU`) deciding whether `raw_bytes`/`raw_bytes_len` or `gpu_buffer`
  is read — same flat-struct-plus-discriminant idiom
  `mediaway-ffi`'s `MediawayDesktopFrame` uses, opposite ownership
  direction (borrowed *input* here vs. borrowed *output* there). See
  [zero-copy/gpu-interop.md](../zero-copy/gpu-interop.md).
- Fresh `mediaway_pipeline_status_t` (13 values, own numbering) — **not**
  named or numbered like `mediaway-ffi`'s `mediaway_status_t`: a
  reused typedef name with a different enumerator set is a real C
  redefinition hazard if a consumer ever includes both headers.
  `mediaway_pipeline_codec_kind_t`, however, **does** mirror
  `mediaway_codec_kind_t`'s numeric values 1:1 (different name, same
  numbers) — both wrap the exact same shared Rust `CodecKind`, so keeping
  the mapping aligned costs nothing and eases a future merge.
- `mediaway_pipeline_ffi_buffer_free(data, len)` — **its own name**, not
  `mediaway_buffer_free`. Real reason: this crate's `Cargo.toml` also builds
  a `staticlib`; two crates exporting the identical global symbol name is a
  guaranteed duplicate-symbol link error if both are ever statically linked
  into one consumer.
- ABI version `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` is `5`: `0`→`1` ADR-0002's struct
  growth, `1`→`2` ADR-0003's audio encode surface, `2`→`3` ADR-0004's decode
  surface, `3`→`4` ADR-0005's capture-to-encode bridge, `4`→`5` ADR-pipeline/0006's
  Opus audio decode + Opus-in-audio-encode.
- `mediaway_decode_session_t` (ADR-0004): single-step, like audio encode — the
  handle IS the decoder, no muxer to wire, no consumption trap.
  `mediaway_auto_video_decode_config_t.extra_data` is required at **open** time
  (not via the first packet — corrected from that ADR's own first draft after
  checking the real `WindowsVideoDecoder::open` contract). CPU output only.
- `mediaway_encode_session_write_frame_from_{camera,desktop}_capture` (ADR-0005):
  polls one frame from a `device`-module capture handle and pushes it straight
  into an encode session — no intermediate frame struct, no extra copy (Screen
  is Zero-Copy end-to-end). First cross-module (`device` handle types accepted
  by `pipeline` functions) coupling in this crate's C ABI.
- `mediaway_audio_decode_session_t` (`adr/pipeline/0006-audio-decode-c-abi.md`):
  single-step, wraps `mediaway_sw::opus::OpusDecoder` **directly** (no `Box<dyn
  Trait>` — no `AudioDecoder` trait exists yet). Reuses
  `mediaway_decode_packet_view_t` for input; an empty payload is Opus
  packet-loss concealment, not an error. Same ADR wires `CodecKind::Opus` into
  the *existing* audio encode surface (`mediaway_audio_encode_config_opus`) —
  `SwOpusAudioEncoder` was already real Rust, just unreachable from C
  (AAC-only dispatch). Both sides cross-platform, round-trip-verified in
  `tests/audio_decode_smoke.rs`.

## Panic safety

Same `catch_unwind` + per-handle `poisoned` pattern as
`mediaway-ffi` (see that crate's wiki page). `AutoEncoderHandle` is
the one deliberate exception — no `poisoned` field, justified above.

## Deferred (see ADR-0001 § Deferred, ADR-0002 § Negative)

`backend`/`max_path_class` config fields (no way to force `Readback`/
`Software` or pin a vendor SDK from C yet); `cbindgen` migration of this
module's `pipeline.h` (tooling adopted crate-wide, this header not yet cut
over — [ADR-0016](../../../adr/0016-cbindgen-ffi-headers.md)'s 2026-08-05 addendum);
`mediaway`'s unconditional decode/device Cargo deps (same class of gap as
`mediaway-container`'s unconditional format-core deps); GPU decode output
(ADR-0004 §1); Microphone audio composed into the same capture-bridge session
(ADR-0005 — still the caller's own job). Decode's own integration test
(`tests/decode_smoke.rs`, mux → demux → decode) was un-`#[ignore]`d — two real
bugs underneath fixed (`WindowsVideoDecoder` AVCC/Annex-B framing + a test
double-free) — see [`platform/windows-decode.md`](../platform/windows-decode.md) § CPU decode bug.

Shared value types (`mediaway_rational_t`, GPU handle types, …) are consolidated into
`include/mediaway/common.h` — no longer a per-header duplication concern, see
[`adr/common/0001-shared-header-consolidation.md`](../../../../crates/mediaway-ffi/adr/common/0001-shared-header-consolidation.md).

`mediaway-ffi` unification — **resolved** by
[ADR-0015](../../../../docs/adr/0015-common-ffi-unification.md): an
`rlib`-only internal crate now shares the `Rational`/`CodecKind` value-type
mirrors and the buffer leak/reclaim helper *implementation* between
`-ffi` crates. Status enums (`mediaway_status_t` vs.
`mediaway_pipeline_status_t`) and each crate's own exported free-function name
stay independent — that independence was judged a feature (zero coordination
needed to link both crates), not a gap to close.
