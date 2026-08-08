# Mediaway v0.1.5

## What's new

### Added

- `mediaway-ffi`: WebM reaches the container C ABI (`mediaway_muxer_create_for_format`/
  `mediaway_demuxer_create_for_format`, ABI v1, `adr/container/0003-multi-format-c-abi.md`)
  — previously the C ABI (and every non-Rust binding) could only open MP4, even though
  `mediaway-container::webm` (real VP8 mux/demux since v0.1.3) has existed with no C-reachable
  path. `MuxerState`/`DemuxerState` become per-format enums; `MediawayStatus` gains
  `UnsupportedCodec`/`UnknownStream`. Verified end-to-end:
  `tests/webm_container_smoke.rs` round-trips 5 VP8 frames through the new functions.
- `mediaway-ffi`: Ogg and ADTS reach the container C ABI via dedicated single-stream handles
  (`mediaway_ogg_muxer_t`/`_demuxer_t`, `mediaway_adts_muxer_t`/`_demuxer_t`, ABI v2 → v3,
  `adr/container/0004-ogg-adts-c-abi.md`) — neither format has track registration or
  `Open`/`Live` typestate, so they don't fit the generic `mediaway_muxer_t`/
  `mediaway_demuxer_t` handles WebM used. Verified end-to-end:
  `tests/ogg_adts_container_smoke.rs` round-trips a real `OpusHead` header + Opus packet and
  two raw AAC frames.
- `mediaway-ffi`: FLV reaches the container C ABI via dedicated `mediaway_flv_muxer_t`/
  `_demuxer_t` handles (ABI v3 → v4, `adr/container/0005-flv-c-abi.md`) — its mux side
  writes tag bytes directly into a caller-supplied buffer on every call (no `poll_bytes`
  step) with a fixed one-video/one-audio track slot, mirroring `flv::Muxer`'s own Rust
  shape. Verified end-to-end: `tests/flv_container_smoke.rs` round-trips one AVC video
  packet and one AAC audio packet, plus unsupported-codec/unregistered-stream rejection.
- `mediaway-ffi`: MPEG-TS reaches the container C ABI via dedicated `mediaway_ts_muxer_t`/
  `_demuxer_t` handles (ABI v4 → v5, `adr/container/0006-mpeg-ts-c-abi.md`) — elementary
  streams are registered at muxer construction (no `add_track`), `write_pat_pmt`/
  `write_access_unit` write directly into a caller-supplied buffer with explicit
  `pts_90k`/`dts_90k` clock values, and `mediaway_ts_demuxer_finish` returns an owned array
  of packets — the only multi-packet demux call in this crate. Verified end-to-end:
  `tests/ts_container_smoke.rs` round-trips one H.264 video and one AAC audio access unit,
  a `finish()`-recovered trailing access unit, and an invalid-PID construction rejection.
- `mediaway-ffi`: MP3 (MPEG Layer III) reaches the container C ABI via dedicated
  `mediaway_mp3_muxer_t`/`_demuxer_t` handles (ABI v5 → v6, `adr/container/0007-mp3-c-abi.md`)
  — a fixed header for the mux session's lifetime (no track registration at all) and an
  explicit `padding` bit on `write_frame`. Verified end-to-end: `tests/mp3_container_smoke.rs`
  round-trips a 128 kbps/44100 Hz stereo frame, a mono channel-count case, and a
  wrong-frame-body-length rejection.
- `mediaway-ffi`: WAV (RIFF/WAVE PCM) reaches the container C ABI (ABI v6 → v7,
  `adr/container/0008-wav-c-abi.md`) — closing out all 8 `mediaway-container` formats.
  `mediaway_wav_muxer_t` is mux-only, since `wav::Muxer::finish` consumes `self` by value;
  demux has no handle at all — `mediaway_wav_parse` is a one-shot whole-buffer function.
  Verified end-to-end: `tests/wav_container_smoke.rs` round-trips PCM and float-format
  frames, a double-`finish()` rejection, and a non-RIFF/WAVE-data rejection.
- C++, C#, Python, and Node.js bindings: all 8 `mediaway-container` formats wired end-to-end
  (WebM/Ogg/ADTS/FLV/MPEG-TS/MP3/WAV joining MP4), each verified against a real native dylib
  reusing shared byte patterns across the four bindings.
- All 4 C-ABI bindings (C++/C#/Python/Node.js) verified on Linux x64, container capability
  only (pure CPU — device/pipeline remain Windows-hardware-verified only). `mediaway-ffi`
  itself needed zero Rust changes: `cargo build -p mediaway-ffi --all-features` already
  produces a clean `libmediaway_ffi.so` exporting all symbols.
- C++, C#, Python, and Node.js bindings: video decode (`DecodeSession`) and Opus audio
  decode (`AudioDecodeSession`) reach all four bindings, closing the last "C ABI real, no
  binding wired" gap — the decode session C ABI (`adr/0004-auto-decode-c-abi.md`,
  `adr/pipeline/0006-audio-decode-c-abi.md`) has existed since v0.1.4. Every binding mirrors
  its existing `AutoVideoEncoder`/`AudioEncoder` single-step-handle shape (the handle IS the
  decoder, no consumption trap, graceful `NoBackend`). Each binding's public `AudioEncoder`
  equivalent stays AAC-only, so the Opus round trip encodes via a raw-ABI path in each
  language instead (C++/C#: raw C ABI / test-local P/Invoke; Node: the `@mediaway/ffi`
  layer directly). Python's `AudioEncoder.open()` was the one exception — it already
  accepted `codec=Codec.OPUS`, needing no workaround. Verified end-to-end in each binding:
  a real WMF H.264 encode→mux→demux→decode round trip (10 frames) and a real Opus
  encode→decode round trip (50 frames) against the real native dylib.

### Fixed

- `mediaway-ffi`: the hand-written `container.h` header's `mediaway_codec_kind_t` was
  missing `MEDIAWAY_CODEC_VP8` — the Rust-side enum has had it since v0.1.3's WebM VP8 work,
  but no C caller could ever name it. Added (`= 12`, matching the existing Rust discriminant).
- `mediaway-ffi`: `mediaway_container_ffi_abi_version()` had drifted to a stale hardcoded
  `0` since the WebM C ABI landed (the header macro had already moved to `1`) — fixed to
  track the real value (`7`, alongside this release's own bumps).
- `mediaway-common`: `CodecKind` had no explicit `#[repr(u8)]`/discriminants at all, found
  while wiring the C++ container bindings — harmless at the FFI boundary itself (the
  `mediaway-ffi`-local mirror enum already had correct explicit values and converts by
  name), but a real latent footgun now pinned explicitly to match the C header's values.
- C++/Python/Node.js bindings: `Muxer`'s auto-assigned track ids started at `0`, silently
  rejected by WebM/Matroska (TrackNumber must not be `0`) though harmless for MP4 — now
  start at `1` for both formats. Only reproducible by linking and running against the real
  dylib, not by `-fsyntax-only` alone.
- Node.js binding: `RawPacket`'s TypeScript interface was missing a `dts` field the
  underlying C ABI already had; `@mediaway/container`/`device`/`encoder`'s internal
  `@mediaway/*` cross-dependency pins were exact-version instead of caret, so npm's
  workspace linker could silently resolve stale published packages instead of local sources.

## Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. The workspace ships 11
freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`,
`flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`,
`mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

## Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified. All 4 C-ABI bindings (C++/C#/Python/Node.js) now verified
  for the container capability on Linux x64 (pure CPU); device/pipeline
  capabilities remain Windows-hardware-verified only.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`, WebCodecs encode
  and decode (`DecodeSession`); encoder/decoder/device crates build for
  wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

## Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`, with
  multi-frame GOP + CBR rate control and live `set_bitrate`), QuickSync (VPL);
  AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video, both
  hardware-verified for GPU decode; AAC — software (ADTS). Auto video
  decode C ABI (CPU output), now reachable from all 4 non-Rust bindings.
- Audio: Opus — Windows decode via Media Foundation and a cross-platform
  software decoder (both behind the `AudioDecoder` trait), software
  encode/decode (`unsafe-libopus`); audio processing module (sonora); AAC —
  software encode (C# `Mediaway.Pipeline.AudioEncoder`, ABI v2). Opus decode
  C ABI now reachable from all 4 non-Rust bindings.
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); `rtp-core` for RTP payloadization (H.264/HEVC).

## Bindings

- C: [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.1.5)
  + CMake/CPack archives (GitHub Release assets) — all 8 `mediaway-container`
  formats (WebM, Ogg, ADTS, FLV, MPEG-TS, MP3, WAV, joining MP4) now reach the
  C ABI (ABI v7); verified building on Linux x64 in addition to Windows.
- C#: [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) packages
  on NuGet (Trusted Publishing, OIDC) — all 8 container formats plus video +
  Opus audio decode (`DecodeSession`/`AudioDecodeSession`) now wired into
  `Mediaway.Pipeline`/`Mediaway.Container`; verified on Linux x64 in addition
  to Windows (container capability).
- Python: [`mediaway`](https://pypi.org/project/mediaway/) on PyPI (Trusted
  Publishing) — all 8 container formats plus video + Opus audio decode now
  wired into the package; verified on Linux x64 in addition to Windows
  (container capability).
- Node: [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi),
  [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container),
  [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device),
  [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder) on
  npm (OIDC Trusted Publishing) — all 8 container formats plus video + Opus
  audio decode now wired into `@mediaway/container`/`@mediaway/encoder`;
  verified on Linux x64 in addition to Windows (container capability).
- C++: all 8 container formats plus video + Opus audio decode
  (`decoder::DecodeSession`/`AudioDecodeSession`) wired into
  `bindings/cpp/include/mediaway/`; verified on Linux x64 in addition to
  Windows (container capability).
- Browser: [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser)
  (wasm, wasm-bindgen).

## Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

## Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines for
every backend. Container mux/demux and video/Opus-audio decode bindings now
reach parity across four ecosystems (C/C++, C#, Python, Node.js) and two
platforms (Windows, Linux x64 for the pure-CPU container capability); device
and pipeline (encode/decode) capabilities remain Windows-hardware-verified
only. Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.
