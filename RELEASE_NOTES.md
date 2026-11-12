# Mediaway v0.2.3

## What's new

### Added

- `mediaway-sw` cargo features `av1` / `h264` / `opus` (default on, so existing consumers build as before): an Opus-only consumer such as a game voice stack no longer builds `rav1e`
- Allocation-free low-level audio entry points in `mediaway-sw` for real-time worker threads: `OpusEncoder::encode_float_into` with runtime setters (bitrate, complexity, DTX, signal, bandwidth, VBR, in-band FEC, packet-loss percent), `OpusDecoder::decode_float_into` (normal decode, PLC and in-band FEC recovery), `AudioProcessor::{push_render_block, process_capture_block, statistics}` and `VoiceActivityDetector::analyze_block`
- `mediaway-device` cargo features `audio` / `camera` / `desktop` (default on) that trim the Linux native dependencies (`pipewire`, `v4l`, `ashpd`) and the Windows camera / desktop backends; a disabled family reports `NotImplemented`

## Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config, and low-level APIs as first-class entry points. The workspace ships freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`, `flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`, `mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and a single C ABI (`mediaway-ffi`).

This release is for Rust consumers that need only part of `mediaway-sw` and `mediaway-device`: codec and device-family cargo features (all default on) and allocation-free Opus, audio-processing and voice-activity entry points for real-time voice. Nothing changes with default features. The C ABI is unchanged from v0.2.2 (container 8, pipeline 7, device 2).

## Platforms

- Windows (win64): primary target and where this release's changes were verified (every `mediaway-device` feature combination built and tested; the new Opus / APM / VAD entry points exercised by a downstream real-time voice stack). The D3D12 native decode paths are unchanged and still deliberately not hardware-run (known TDR on the existing D3D12 H.264 decode path).
- Linux: the `audio` / `camera` / `desktop` features can drop `pipewire`, `v4l` and `ashpd`; CI builds default features, single-family builds were not compiled. VA-API, DMA-BUF Zero-Copy and the AMF encode backend are compile/test-verified on WSL2 only, with no real VA-API or AMD GPU hardware available.
- macOS / iOS: unchanged this release (not trimmed by the new features). The Apple AAC path is the only Apple backend that has run on a real Mac in CI; every other Apple backend (VideoToolbox H.264/HEVC/VP9/AV1/ProRes, camera, screen, Opus) is still compile- and lint-verified only.
- Android: unchanged (not trimmed by the new features). NDK `AMediaCodec` decode and Camera2/AAudio/`MediaProjection` capture remain authored without a device or emulator, and are not in CI.
- Web (wasm32): unchanged. `@mediaway/browser` ships `iso-bmff-wasm` and WebCodecs encode/decode, compile-verified only.

## Codecs

- Encode: H.264 — NVENC, Vulkan Video, QuickSync (VPL), VA-API (GOP), AMF,
  Apple (unverified); HEVC — VA-API (GOP), AMF, Apple (unverified) and, on
  Windows, muxed into playable MP4; VP9 — VA-API (narrow real-driver support,
  untested); AV1 — software (rav1e, now behind the `av1` feature), AMF (untested),
  Vulkan (implemented but driver-blocked on this workspace's reference GPU);
  ProRes — Apple (unverified).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (hardware-verified),
  VA-API (GOP, untested), D3D12 (HEVC, sans-io only), Apple (unverified); VP9 —
  VA-API (untested), Apple (unverified); AV1 — Vulkan (keyframe-only,
  hardware-verified), VA-API (keyframe-only, untested), D3D12 (keyframe-only,
  sans-io only), Apple (unverified); ProRes — Apple (unverified).
- Audio: Opus — Windows decode via Media Foundation, cross-platform software
  encode/decode (`unsafe-libopus`, behind the `opus` feature, now with
  allocation-free float entry points, PLC and in-band FEC recovery), native Apple
  encode/decode (unverified for Opus), and a real `Opus`/`dOps` MP4 track; AAC —
  Windows encode and decode via Media Foundation (hardware-verified round trip),
  software encode (C# `AudioEncoder`), Apple encode/decode (verified on a real
  macOS runner: round trip to PCM); audio processing module (sonora: AEC3 / NS /
  AGC2 / VAD, now with allocation-free block-level entry points). AAC is the OS's
  own decoder, so its samples can differ between hosts, unlike the software Opus
  decoder.
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); `rtp-core` for RTP payloadization (H.264/HEVC).

## Bindings

The native bindings (C, C#, Python, Node, C++) and `@mediaway/browser` carry no API change in this release: the C ABI is unchanged, and the packages are re-stamped with 0.2.3 from the workspace version. [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.2.3) and the CMake/CPack archives are on the GitHub Release page; [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) on NuGet, [`mediaway`](https://pypi.org/project/mediaway/) on PyPI and [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi), [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container), [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device), [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder), [`@mediaway/decoder`](https://www.npmjs.com/package/@mediaway/decoder), [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser) on npm. The Rust crates are on [crates.io](https://crates.io/crates/mediaway) (including [`mediaway-sw`](https://crates.io/crates/mediaway-sw) and [`mediaway-device`](https://crates.io/crates/mediaway-device)).

## Breaking changes

None. The new cargo features are all default-on, so a consumer with default features builds and behaves exactly as with v0.2.2; every other addition is a new function or method.

## Maturity bar

Not production-ready. The changes in this release were verified on Windows (build and tests for every `mediaway-device` feature combination; the Opus / APM / VAD entry points under a downstream voice stack's link, echo-cancellation and allocation tests); treat every path without an explicit "hardware-verified" tag as unverified. The exceptions to read twice are unchanged from v0.2.2:

- **Apple VideoToolbox, camera, screen and Opus, Android, Linux VA-API/AMF and the D3D12 decode paths** are authored and compile- or test-verified only, with no real-hardware run. The Apple AAC path is the only Apple backend that has run on a real Mac in CI.
- **Bindings on Linux and macOS** are verified for the container capability at most.
- **AAC decode output is host-dependent** by construction: it is the OS's codec.
- **H.264/HEVC CPU decode on Windows** is the inbox Media Foundation software decoder, which has quirks (crop offsets, parameter sets, held-back frames) that v0.2.2 works around; a stream shape not in the tests may expose another.

Costly paths (CPU readback, SW fallbacks, the throwaway sessions behind the capability probes, a window region away from the origin) are documented at each API (`docs/spec/caveats-and-clarity.md`). See `docs/spec/status.md`.
