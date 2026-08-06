# Changelog

All notable changes to Mediaway, grouped by release. Development changes
accumulate under `## Unreleased` in `RELEASE_NOTES.md` and are finalized here
at release time (`/release-notes <version>`). The most recent section is also
the skeleton source for the next release note (Overview / Platforms / Codecs /
Bindings / Maturity bar).

## [0.1.4] - 2026-08-07

### What's new since 0.1.3

#### Added

- Video decode C ABI (`mediaway_decode_session_open/push_packet/poll_frame/flush/close`) wrapping the auto video decoder — CPU output only this pass
- Capture-to-encode bridge C ABI — a polled Camera/Screen capture frame pushes straight into an encode session with no extra copy (Screen is Zero-Copy end-to-end)
- Opus audio decode C ABI, and `CodecKind::Opus` wired into the existing audio encode C ABI (previously AAC-only)
- Vulkan H.264/HEVC encode: multi-frame GOP (P-frame prediction) plus CBR rate control for H.264, hardware-verified on an RTX 4090
- `VideoEncoder::set_bitrate` — live CBR bitrate retargeting mid-session with no reopen, implemented for Vulkan H.264
- `AudioDecoder` trait in `mediaway-decoder`, implemented by the WMF Opus decoder and a cross-platform software Opus decoder
- New freestanding sans-io crate `rtp-core` — RTP payloadization for H.264/HEVC (RFC 3550/6184/7798), closing the workspace's previous no-RTP gap
- `@mediaway/browser`: `DecodeSession` — demux-then-decode fMP4 playback via the browser's native WebCodecs decoders, the decode-side mirror of `EncodeSession`
- FFI + C#: GOP/CBR encode config and live `set_bitrate` now reach the C ABI (`mediaway_auto_video_encode_config_t`, ABI v5 → v6) and the `Mediaway.Pipeline` C# package
- D3D12 native video-encode backend (internal, not yet wired into the public API) gains GOP support and row-based intra-refresh for H.264/HEVC

#### Changed

- FFI: shared C header value types (`mediaway_rational_t`, pixel/sample formats, GPU handles) consolidated into a new `include/mediaway/common.h`
- FFI: adopted `cbindgen` tooling for header generation; the shipped headers stay hand-written pending a follow-up migration

#### Fixed

- Vulkan HEVC GPU decode no longer produces an all-zero picture — a missing PPS slice-header flag was desyncing the driver's CABAC parser
- Windows CPU H.264 decode silently produced zero frames for Annex-B streams from a WMF encoder — now decodes correctly
- D3D12 native H.264 decode (internal): `BitOffsetToSliceData` corrected per the official DXVA spec
- D3D12 native AV1 encode (internal): fixed a feature-query bug plus DPB-index, buffer-size, and subregion-metadata bugs — output is now structurally valid, though real hardware decode verification is still open
- FFI: fixed a double-free crash during decode session teardown

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. The workspace ships 11
freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`,
`flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`,
`mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770 — Vulkan H.264 **and** HEVC GPU decode both hardware-verified
  this release.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`, WebCodecs encode
  **and** decode (`DecodeSession`); encoder/decoder/device crates build for
  wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`, now with
  multi-frame GOP + CBR rate control and live `set_bitrate`), QuickSync (VPL);
  AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video, both
  hardware-verified for GPU decode; AAC — software (ADTS). New auto video
  decode C ABI (CPU output).
- Audio: Opus — Windows decode via Media Foundation and a new cross-platform
  software decoder (both behind the new `AudioDecoder` trait), software
  encode/decode (`unsafe-libopus`); audio processing module (sonora); AAC —
  software encode (C# `Mediaway.Pipeline.AudioEncoder`, ABI v2).
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); new `rtp-core` for RTP payloadization (H.264/HEVC).

### Bindings

- C: `mediaway_ffi.h` + CMake/CPack archives — video decode, capture-to-encode
  bridge, and Opus audio decode/encode all newly reachable this release.
- C#: `Mediaway.*` packages on NuGet (Trusted Publishing, OIDC) — GOP/CBR
  encode config and live `SetBitrate` newly reachable this release.
- Python: `mediaway` on PyPI (Trusted Publishing).
- Node: `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`,
  `@mediaway/encoder` on npm (OIDC Trusted Publishing).
- Browser: `@mediaway/browser` (wasm, wasm-bindgen) — now decode-capable via
  `DecodeSession`, not just encode.

### Breaking changes

`mediaway-ffi`'s pipeline C ABI version bumped 5 → 6 (new
`mediaway_auto_video_encode_config_t` fields, new
`mediaway_encode_session_set_bitrate` export) — recompile any C/C++ caller
against the updated header. Pre-1.0; APIs may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines for
every backend. GOP/CBR/live-bitrate-retargeting reach the FFI/C# surface this
release but are honestly scoped: the auto-selected backend they resolve to
today (WMF on Windows) does not yet implement them, so those fields are a
documented no-op through that path — only the standalone Vulkan encoders
honor them. Costly paths (CPU readback, SW fallbacks) are documented at each
API (`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

## [0.1.3] - 2026-08-05

### What's new since 0.1.2

#### Added

- C#: `Mediaway.Pipeline.AudioEncoder` — AAC audio encode (ABI v2), matching the existing Node.js `@mediaway/encoder` capability
- C#: `Device/CaptureMicrophone.cs` and `Pipeline/EncodeAudio.cs` examples; existing examples reorganized under `Container/`/`Device/`/`Pipeline/` to mirror the Node.js binding's example layout
- `ebml-webm`: `Muxer::push_laced_frames` — EBML lacing on the mux side (previously demux-only)
- `CodecKind::Vp8`, wired into `mediaway-container::webm` mux + demux — closes the WebM VP8 gap

#### Changed

- `ebml-webm` demux: indefinite-size `Cluster` sibling-ID lookahead — the open-element stack no longer grows unboundedly on a long-running live `WebM` stream

#### Fixed

- `ebml-webm` mux output is now verified against system `ffprobe` in addition to this crate's own demuxer round-trip

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. This release ships the
consolidated workspace (ADR-0021): 10 freestanding, independently versioned
core crates (`iso-bmff`, `ebml-webm`, `flv-core`, `adts-core`, `ogg-core`,
`riff-wave-core`, `mpeg-ts-core`, `mpeg-audio`, `iso-cenc`, `rtmp`) plus one
`mediaway` umbrella with five capability crates (`container`, `encoder`,
`decoder`, `device`, `sw`) and a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`; encoder/decoder/
  device crates build for wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`),
  QuickSync (VPL); AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (sans-io SPS/PPS/
  slice parsing is unit-tested; GPU decode hardware-verified for H.264);
  AAC — software (ADTS).
- Audio: Opus — Windows decode via Media Foundation (public
  `mediaway_decoder::windows::WmfOpusDecoder`), software encode/decode
  (`unsafe-libopus`); audio processing module (sonora); AAC — software encode
  (C# `Mediaway.Pipeline.AudioEncoder`, ABI v2).
- Containers: ISOBMFF/MP4, WebM (EBML, now including VP8 mux/demux and
  mux-side lacing), FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG audio — all
  verified playable in mpv; CENC encryption/decryption; RTMP (proposed,
  unpublished).

### Bindings

- C: `mediaway_ffi.h` + CMake/CPack archives.
- C#: `Mediaway.*` packages on NuGet (Trusted Publishing, OIDC).
- Python: `mediaway` on PyPI (Trusted Publishing).
- Node: `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`,
  `@mediaway/encoder` on npm (OIDC Trusted Publishing).
- Browser: `@mediaway/browser` (wasm, wasm-bindgen).

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines.
Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

## [0.1.2] - 2026-08-04

### What's new since 0.1.1

#### Added

- Windows Opus decode via Media Foundation (public API) and software Opus encode in the facade

#### Changed

- All 8 container formats (audio included) verified playable in mpv via the playback-verification example
- `flv` and `ogg` freestanding cores renamed to `flv-core` / `ogg-core` (crates.io name collisions)
- `ebml-webm` 0.2.0 — CodecPrivate API
- All crates re-published on crates.io (workspace 0.1.2, freestanding cores 0.1.1, `ebml-webm` 0.2.1) with refreshed READMEs and consumer-facing descriptions

#### Fixed

- MP4s that failed to play: malformed `stsz` box and raw SPS written as `avcC`
- Playback timing corrections: ISO-BMFF mux duration/DTS delta (ADR-0004) and Ogg demux/CRC fixes

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. This release ships the
consolidated workspace (ADR-0021): 10 freestanding, independently versioned
core crates (`iso-bmff`, `ebml-webm`, `flv-core`, `adts-core`, `ogg-core`,
`riff-wave-core`, `mpeg-ts-core`, `mpeg-audio`, `iso-cenc`, `rtmp`) plus one
`mediaway` umbrella with five capability crates (`container`, `encoder`,
`decoder`, `device`, `sw`) and a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`; encoder/decoder/
  device crates build for wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`),
  QuickSync (VPL); AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (sans-io SPS/PPS/
  slice parsing is unit-tested; GPU decode hardware-verified for H.264);
  AAC — software (ADTS).
- Audio: Opus — Windows decode via Media Foundation (public
  `mediaway_decoder::windows::WmfOpusDecoder`), software encode/decode
  (`unsafe-libopus`); audio processing module (sonora).
- Containers: ISOBMFF/MP4, WebM (EBML), FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE,
  MPEG audio — all 8 verified playable in mpv; CENC encryption/decryption;
  RTMP (proposed, unpublished).

### Bindings

- C: `mediaway_ffi.h` + CMake/CPack archives.
- C#: `Mediaway.*` packages on NuGet (Trusted Publishing, OIDC).
- Python: `mediaway` on PyPI (Trusted Publishing).
- Node: `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`,
  `@mediaway/encoder` on npm (OIDC Trusted Publishing).
- Browser: `@mediaway/browser` (wasm, wasm-bindgen).

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines.
Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

## [0.1.1] - 2026-08-03

First release (0.1.0 was a manual npm-only publish; this is the first
cross-registry release). Early pre-1.0 snapshot — see the maturity bar below
before relying on any API.

### What's new since 0.1.0

- npm packages now ship READMEs with runnable examples + consumer-facing
  descriptions; NuGet packages carry a shared README
- crates.io: 19-crate dependency-ordered publish (9 freestanding cores +
  mediaway family + avcli/avprobe + vpl-sys; colliding names published as
  adts-core / mpeg-ts-core / riff-wave-core)
- release pipeline: OIDC Trusted Publishing for npm/NuGet/PyPI (no tokens),
  branch ruleset + environment approval gate on release branches

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. This release ships the
consolidated workspace (ADR-0021): 10 freestanding, independently versioned
core crates (`iso-bmff`, `ebml-webm`, `flv`, `adts`, `ogg-core`, `riff-wave`,
`mpeg-ts`, `mpeg-audio`, `iso-cenc`, `rtmp`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`; encoder/decoder/
  device crates build for wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`),
  QuickSync (VPL); AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (sans-io SPS/PPS/
  slice parsing is unit-tested; GPU decode hardware-verified for H.264);
  AAC — software (ADTS).
- Audio: Opus encode/decode (software), audio processing module (sonora).
- Containers: ISOBMFF/MP4, WebM (EBML), FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE,
  MPEG audio; CENC encryption/decryption; RTMP (proposed, unpublished).

### Bindings

- C: `mediaway_ffi.h` + CMake/CPack archives.
- C#: `Mediaway.*` packages on NuGet (Trusted Publishing, OIDC).
- Python: `mediaway` on PyPI (Trusted Publishing).
- Node: `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`,
  `@mediaway/encoder` on npm (OIDC Trusted Publishing).
- Browser: `@mediaway/browser` (wasm, wasm-bindgen).

### Breaking changes

None — first release. Note for early adopters of the pre-consolidation layout:
`mediaway-pipeline` was renamed `mediaway` (ADR-0021) and platform backend
crates became `#[cfg]`-gated modules inside their capability crate. APIs are
pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines.
Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.
