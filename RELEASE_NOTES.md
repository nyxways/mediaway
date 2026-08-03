# Mediaway v0.1.0

First public release. This is an early pre-1.0 snapshot — see the maturity bar
below before relying on any API.

## Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. This release ships the
consolidated workspace (ADR-0021): 10 freestanding, independently versioned
core crates (`iso-bmff`, `ebml-webm`, `flv`, `adts`, `ogg`, `riff-wave`,
`mpeg-ts`, `mpeg-audio`, `iso-cenc`, `rtmp`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

## Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`; encoder/decoder/
  device crates build for wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

## Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`),
  QuickSync (VPL); AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (sans-io SPS/PPS/
  slice parsing is unit-tested; GPU decode hardware-verified for H.264);
  AAC — software (ADTS).
- Audio: Opus encode/decode (software), audio processing module (sonora).
- Containers: ISOBMFF/MP4, WebM (EBML), FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE,
  MPEG audio; CENC encryption/decryption; RTMP (proposed, unpublished).

## Bindings

- C: `mediaway_ffi.h` + CMake/CPack archives.
- C#: `Mediaway.*` packages on NuGet (Trusted Publishing, OIDC).
- Python: `mediaway` on PyPI (Trusted Publishing).
- Node: `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`,
  `@mediaway/encoder` on npm (OIDC Trusted Publishing).
- Browser: `@mediaway/browser` (wasm, wasm-bindgen).

## Breaking changes

None — first release. Note for early adopters of the pre-consolidation layout:
`mediaway-pipeline` was renamed `mediaway` (ADR-0021) and platform backend
crates became `#[cfg]`-gated modules inside their capability crate. APIs are
pre-1.0 and may change without a major bump.

## Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines.
Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.
