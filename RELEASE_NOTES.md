# Mediaway v0.2.1

## What's new

### Added

- Window capture over the C ABI (C, C++, C#, Python, Node.js): record one window with a chosen pointer, a cropped region, a hidden capture border and even-cropped frames, Windows only
- Streaming fMP4 bytes over the pipeline C ABI (`mediaway_encode_session_poll_bytes`) so a long capture's memory is bounded by poll cadence
- AAC decode over the pipeline C ABI (Windows Media Foundation, Apple AudioToolbox) given the stream's `AudioSpecificConfig`
- macOS CI runs the Apple audio backends on a real runner for the first time, instead of only linting them
- Encoder and decoder capability probes over the pipeline C ABI (`mediaway_encoder_support_at`, `mediaway_decoder_support`), both costly because they open throwaway sessions
- Replay ring over the container C ABI: keep the last N seconds of encoded packets and cut a standalone clip at a keyframe, holding payloads or only file locations
- MP4 payload placements over the container C ABI (`mediaway_muxer_create_with_placements`) to find every sample's bytes in the output file
- `mediaway_container::replay::ReplayRing<P>` in Rust, generic over `Bytes` or `StoredPayload`, cutting in decode order so B-frames are never stranded
- `iso_bmff::Muxer::with_placements` and `poll_placements`, off by default and free when off
- `VideoEncoder::finish(self)` and `AudioEncoder::finish(self)` to flush and collect every remaining packet, with the cost of skipping it documented
- `mediaway::platform::WindowCapture` and `platform::DesktopAudio` through the auto-dispatch facade, returning concrete per-target types
- Capture a region of a window with `DesktopVideoCaptureConfig::region`, cropped by the frame pool or by one GPU copy per frame
- Windows AAC decode with `WmfAacDecoder`, a sample-exact round trip against the Windows AAC encoder
- `EncodeSession::open_in` and `open_in_with_audio` to write any container through the facade, plus `MuxOpen`, `ContainerError` and `Mux::set_track_extra_data`
- `EncodeSession::poll_bytes` and `finish_into` to drain fMP4 bytes during a session
- `encoder_support_at` and `mediaway_encoder::windows::auto::support_at` to probe encoder availability at the resolution you will use

### Changed

- `EncodeSession` is generic over its muxer with `mp4::Muxer` as the default, so existing call sites are unchanged
- `EncodeSession::finish` wraps `finish_into` and returns only the unpolled tail after `poll_bytes`

### Fixed

- The Apple AAC decoder never opened on a real Mac (`AudioConverter` answered `'!dat'` to the bare `AudioSpecificConfig` it was given as the magic cookie); it now wraps it in the `esds` descriptor Core Audio requires and decodes a real stream back to PCM
- The Apple AAC encoder handed out Core Audio's 39-byte `esds` descriptor as the stream's `extra_data`, so an MP4 track built from it wrapped the descriptor twice; it now exposes the bare two-byte `AudioSpecificConfig`
- HEVC in MP4 was never playable: the `hvcC` record, the Annex-B to length-prefixed conversion and the Windows encoder's missing `hvcC` are all fixed, including on macOS
- HEVC read back out of an MP4 decoded zero frames on Windows and now decodes every frame
- Opus in MP4 was written as AAC and is now a real `Opus`/`dOps` track; files written by earlier versions are invalid and must be remuxed
- A Windows H.264 frame with an unknown duration (`duration: 0`) scrambled the timeline and dropped frames on playback; it is now one time-base tick
- Windows video timestamps did not survive the encoder: the hns round trip is exact, `dts` comes from the MFT, and a variable-frame-rate recording declares its real length
- Dropping a Windows hardware video encoder could crash the process; it now waits a measured 50 ms before releasing an async MFT
- DX11 Zero-Copy encode now works on NVIDIA hardware, where three async-MFT sequencing bugs made it fail on every machine
- WGC window capture can hide its border and trim odd-sized frames to even, which hardware encoders require
- Windows per-process audio loopback never opened on any machine and now works
- `ProcessTreeScope::ProcessOnly` recorded the inverse of what it promised and is renamed `ExcludeProcessTree`
- The encoder capability probe no longer reports `NoDevice` for backends that work
- `cargo nextest run --workspace` no longer takes over the developer's desktop

### Breaking

- `MEDIAWAY_DEVICE_FFI_ABI_VERSION` 1 to 2: `mediaway_desktop_capture_config_t` grew, zero-initialised configs behave as before, and Screen configs now refuse a pointer or region on DXGI
- `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` 6 to 7: `mediaway_audio_decode_config_t` gained `extra_data` and `extra_data_len`, both null for Opus
- `MEDIAWAY_CONTAINER_FFI_ABI_VERSION` 7 to 8: additions only (replay ring, placements), no existing struct changed
- `DesktopVideoCaptureConfig` gains `region` and `cursor`, so struct literals must add them; macOS previously always showed the pointer and now hides it by default
- `ProcessTreeScope::ProcessOnly` is renamed `ExcludeProcessTree`, and the C ABI's `include_child_processes` is renamed `include_target_process_tree` with the same polarity and layout
- `CaptureError` gains `RegionOutOfBounds` and `BackendCode { code }`, and `PipelineError::Mux` now wraps `ContainerError` instead of `mp4::Error`
- `iso-bmff` writes HEVC and Opus tracks differently, so files those tracks produced with earlier versions are invalid

## Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. The workspace ships 11
freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`,
`flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`,
`mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`). This release brings the non-Rust bindings up
to what the Rust API gained since v0.1.8 (window capture, streaming, AAC decode,
capability probes, the replay ring) and fixes a run of Windows encode/decode
defects found by testing the container round trip end to end. `iso-bmff` moves
to 0.1.2 because the HEVC and Opus fixes and the new placements API live there.

`v0.2.0` reached crates.io only. Its release pipeline stopped at the macOS RC gate, before any npm,
NuGet, PyPI or GitHub release existed, because the Apple AAC decoder did not open on a real
Mac and the Apple AAC encoder exposed the wrong `extra_data` (both under **Fixed**). Both were
authored without ever running, which is why nothing had noticed since v0.1.8. Everything shipped
as `0.2.1`, the same way v0.1.8 followed a v0.1.7 that got no further than crates.io. The 0.2.0
crates on crates.io are complete but contain those two defects; use 0.2.1.

## Platforms

- Windows (win64): primary target and where this release's changes were verified,
  on an RTX 4090 and Intel UHD 770. DX11 Zero-Copy encode now works on NVIDIA's
  async encoder MFTs, AAC can be decoded as well as encoded, and window capture
  can hide the WGC border, choose the pointer, trim to even sizes and crop a
  region. The D3D12 native decode paths are unchanged and still deliberately not
  hardware-run (known TDR on the existing D3D12 H.264 decode path).
- Linux: unchanged this release. VA-API, DMA-BUF Zero-Copy and the AMF encode
  backend are compile/test-verified on WSL2 only, with no real VA-API or AMD GPU
  hardware available. A capture region is refused with `Unsupported`.
- macOS / iOS: AAC now runs on a real Mac. On a `macos-14` runner the Apple AAC decoder opens and
  decodes, the encoder exposes the bare `AudioSpecificConfig`, and an encode-to-decode round trip
  returns real PCM (40 packets to 40960 samples, mean square 0.474 on a unit sine); CI now runs
  those tests. Also changed: the HEVC `hvcC` fix corrects every HEVC file produced there (it
  shared the broken builder), and the macOS capture pointer is hidden by default. **Every other
  Apple backend (VideoToolbox H.264/HEVC/VP9/AV1/ProRes, camera, screen, Opus) is still compile-
  and lint-verified only and has never run in CI; this release showed what that is worth**, so
  assume the same class of defect until each is run.
- Android: unchanged. NDK `AMediaCodec` decode and Camera2/AAudio/`MediaProjection`
  capture remain authored without a device or emulator, and are not in CI.
- Web (wasm32): unchanged. `@mediaway/browser` ships `iso-bmff-wasm` and WebCodecs
  encode/decode, compile-verified only; the Opus-in-MP4 fix applies to its muxer.

## Codecs

- Encode: H.264 — NVENC, Vulkan Video, QuickSync (VPL), VA-API (GOP), AMF,
  Apple (unverified); HEVC — VA-API (GOP), AMF, Apple (unverified) and, on
  Windows, now muxed into playable MP4; VP9 — VA-API (narrow real-driver support,
  untested); AV1 — software (rav1e), AMF (untested), Vulkan (implemented but
  driver-blocked on this workspace's reference GPU); ProRes — Apple (unverified).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (hardware-verified),
  VA-API (GOP, untested), D3D12 (HEVC, sans-io only), Apple (unverified); HEVC
  read back out of an MP4 now decodes on Windows; VP9 — VA-API (untested), Apple
  (unverified); AV1 — Vulkan (keyframe-only, hardware-verified), VA-API
  (keyframe-only, untested), D3D12 (keyframe-only, sans-io only), Apple
  (unverified); ProRes — Apple (unverified).
- Audio: Opus — Windows decode via Media Foundation, cross-platform software
  encode/decode (`unsafe-libopus`), native Apple encode/decode (unverified for Opus), and a
  real `Opus`/`dOps` MP4 track; AAC — Windows encode and now decode via Media
  Foundation (hardware-verified round trip), software encode (C# `AudioEncoder`),
  Apple encode/decode (verified on a real macOS runner: round trip to PCM); audio processing module (sonora). AAC decode
  and the Opus decode session are both reachable from all five native bindings
  through one C ABI call, but AAC is the OS's own decoder, so its samples can
  differ between hosts, unlike the software Opus decoder.
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); `rtp-core` for RTP payloadization (H.264/HEVC). New:
  a replay ring over encoded packets and MP4 payload placements.

## Bindings

Every package below ships native libs for Windows x64, Linux x86_64 and macOS
(x86_64 + arm64) (ADR-0024). Linux is verified for the container capability only;
the device and pipeline capabilities, including everything new in this release,
are Windows-hardware-verified, and the macOS gate ran the C#, Python, Node and C round trips
on a real Apple Silicon runner (the pipeline capability there is Apple AAC and VideoToolbox). Window capture, streaming, AAC decode, the probes and the
replay ring reached every binding below and were re-run against the real native library.
The C ABI still has no video-packet source, so a replay ring is fed demuxer packets,
audio-encoder packets or packets from an encoder you drive yourself.

- C: [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.2.1)
  + one CMake/CPack archive per platform (GitHub Release assets) — device ABI 2,
  pipeline ABI 7, container ABI 8; recompile against the new headers.
- C#: [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) packages
  on NuGet (Trusted Publishing, OIDC) — window capture, `PollBytes`, AAC decode,
  probes and `ReplayRing`/`ReplayClip`.
- Python: [`mediaway`](https://pypi.org/project/mediaway/) on PyPI (Trusted
  Publishing) — the same surface; `include_child_processes` is now
  `include_target_process_tree`.
- Node: [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi),
  [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container),
  [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device),
  [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder),
  [`@mediaway/decoder`](https://www.npmjs.com/package/@mediaway/decoder) on
  npm (OIDC Trusted Publishing) — the same surface.
- C++: `bindings/cpp/include/mediaway/` — `WindowCapture`, `pollBytes`, `openAac`,
  `encoderSupport`/`decoderSupport` and `ReplayRing`/`ReplayClip`; header-only.
- Browser: [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser)
  (wasm, wasm-bindgen) — unchanged apart from the Opus-in-MP4 fix in its muxer.

## Breaking changes

This release changes three C ABI versions and several Rust types. All are
pre-1.0 and may change without a major bump, but you will notice these:

- **Recompile against the new headers.** Device ABI 2 and pipeline ABI 7 grew
  by-value config structs (zero-initialised new fields behave as before). Container
  ABI 8 only adds functions.
- **Struct literals of `DesktopVideoCaptureConfig`** must add `region` and `cursor`;
  the `::screen`/`::window` constructors set them.
- **Renames:** `ProcessTreeScope::ProcessOnly` to `ExcludeProcessTree` and the C ABI's
  `include_child_processes` to `include_target_process_tree` (same layout, so only
  source references change).
- **`CaptureError` and `PipelineError`:** two new `CaptureError` variants, and
  `PipelineError::Mux` wraps `ContainerError`.
- **Files written by earlier versions with an HEVC or Opus track in MP4 are
  invalid** and must be remuxed.

## Maturity bar

Not production-ready. Everything new in this release was verified on **Windows
hardware through the real native library** (C ABI and each binding), and those
tests are named in the notes above; treat every path without an explicit
"hardware-verified" tag as unverified. The exceptions are the ones to read twice:

- **The Apple audio backends were unrun until this release and were broken.** AAC decode and
  AAC encode both had a defect that only a real Mac could show, found by the release
  pipeline's RC gate rather than by any test. They are fixed and verified on a `macos-14`
  runner (Apple Silicon), and CI now runs them. Treat this as evidence about the rest of the
  Apple surface, which has no such run.
- **Apple VideoToolbox, Android, Linux VA-API/AMF and the D3D12 decode paths** are authored and
  compile- or test-verified only, with no real-hardware run, as in v0.1.8.
- **Bindings on Linux and macOS** are verified for the container capability at most;
  the new device and pipeline surface was exercised on Windows only.
- **AAC decode output is host-dependent** by construction: it is the OS's codec.
- **The replay ring has no C video-packet source yet**, so from C it is fed
  demuxed, audio-encoder or caller-encoded packets.

A run of these fixes (HEVC in MP4, Opus in MP4, unknown frame duration) were
defects that no existing test could see, because those tests never put the
container in the loop. The tests added with them do. Costly paths (CPU readback,
SW fallbacks, the throwaway sessions behind the capability probes, a window
region away from the origin) are documented at each API
(`docs/spec/caveats-and-clarity.md`). See `docs/spec/status.md`.
