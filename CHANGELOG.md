# Changelog

All notable changes to Mediaway, grouped by release. Development changes
accumulate under `## Unreleased` in `RELEASE_NOTES.md` and are finalized here
at release time (`/release-notes <version>`). The most recent section is also
the skeleton source for the next release note (Overview / Platforms / Codecs /
Bindings / Maturity bar).

## [0.1.6] - 2026-08-11

### What's new since 0.1.5

#### Added

- `mediaway-device`: GPU adapter enumeration (`windows::enumerate_gpu_adapters`) and a
  configurable DirectX11 device factory (`windows::GpuDevice`) — the first reusable way
  for a caller without a pre-existing GPU device to get a real `GpuDeviceHandle` for
  Zero-Copy capture/encode/decode paths
- `mediaway-ffi`: C ABI for the GPU device factory (`mediaway_gpu_adapter_list`,
  `mediaway_gpu_device_create`/`_handle`/`_close`) — the first way for a non-Rust
  caller to create a real GPU device and drive Screen capture / GPU-input encode from
  outside Rust; `bindings/c/examples/device/capture_screen.c` and
  `bindings/c/examples/pipeline/screen_record.c` now link+run-verify real Screen
  capture on real hardware instead of only demonstrating the gap
- `@mediaway/decoder` (Node.js): new npm package — video decode + Opus audio decode,
  split out of `@mediaway/encoder`'s previously undiscoverable `decode.ts`
- `@mediaway/device` (Node.js): the GPU device factory (`listGpuAdapters`,
  `GpuDevice`) and real Screen capture — `openScreenCapture()` creates a GPU device
  internally (or accepts a caller-supplied one) instead of always throwing
  `CaptureUnavailableError`
- `@mediaway/encoder` (Node.js): the capture-to-encode bridge
  (`EncodeSession.writeFrameFromCameraCapture`/`writeFrameFromDesktopCapture`) and
  `AutoVideoEncodeConfig.gpuDevice` for Zero-Copy GPU input — `examples/device/capture-screen.ts`
  and `examples/pipeline/screen-record.ts` now run-verify real Screen capture on real
  hardware instead of only demonstrating the gap
- `Mediaway.Device` (C#): the GPU device factory (`GpuDevice.ListAdapters`/`Create`/
  `TryCreate`) — the first way for a C# caller to construct a real `ID3D11Device` without
  raw COM interop
- `Mediaway.Pipeline` (C#): the capture-to-encode bridge
  (`EncodeSession.WriteFrameFromCameraCapture`/`WriteFrameFromDesktopCapture`) —
  `ScreenRecord.cs` now builds its GPU device via the new factory and streams through the
  bridge instead of a `NotImplementedException` placeholder, and
  `Mediaway.Device.Tests`/`Mediaway.Pipeline.Tests` hardware-verify real Screen capture and
  the bridge on real hardware instead of a hand-rolled test-only `D3D11CreateDevice`
- `mediaway` (Python): `GpuDevice.list_adapters()`/`create()` and the capture-to-encode
  bridge (`EncodeSession.write_frame_from_camera_capture`/`write_frame_from_desktop_capture`)
  — `VideoCapture.open(source="screen")` now opens real GPU-backed Screen capture instead
  of always raising `CaptureUnsupportedError`; `examples/device/capture_screen.py` and
  `examples/pipeline/screen_record.py` now run-verify real Screen capture on real hardware
  instead of only demonstrating the gap
- `mediaway::device::GpuDevice` (C++): `listAdapters()`/`create()` — the first way for a
  C++ caller to construct a real `ID3D11Device` without raw COM interop.
  `mediaway::device::ScreenCapture::open()` now opens real Zero-Copy Screen capture
  (`ScreenCaptureConfig::gpuDevice`) instead of always throwing `Status::Unsupported`
- `mediaway::encoder::EncodeSession` (C++): the capture-to-encode bridge
  (`writeFrameFromCameraCapture`/`writeFrameFromDesktopCapture`) —
  `examples/device/capture_screen.cpp` and `examples/pipeline/screen_record.cpp` now
  link+run-verify real Screen capture and the bridge on real hardware instead of only
  demonstrating the gap. This completes GPU device factory + Screen capture +
  capture-to-encode bridge parity across every planned binding (C, Node.js, C#, Python, C++)

#### Fixed

- `mediaway-ffi`: `mediaway_pipeline_ffi_abi_version()` was still returning `5` while
  `include/mediaway/pipeline.h`'s `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` had already been
  bumped to `6` — every C caller's own ABI-version self-check was silently failing
- `bindings/nodejs`: `bun install`'s default per-package/isolated workspace linking
  did not hoist `@mediaway/*` into the root `node_modules`, breaking root-level
  `tsc --noEmit`/`tsx` resolution for `test/*.ts` (pre-existing, reproduced even on
  files untouched by this change) — fixed via `bunfig.toml`'s `[install] linker = "hoisted"`
- `bindings/csharp`: `ScreenRecord.cs`'s `DrainAudioAsync` had an ambiguous bare
  `AudioFrame` reference between `Mediaway.Device.Audio.AudioFrame` and
  `Mediaway.Pipeline.AudioFrame` (pre-existing, only surfaced once `Mediaway.Pipeline`
  referenced `Mediaway.Device.Camera`/`Mediaway.Device.Desktop` for the bridge above) —
  fixed by fully qualifying the type
- `bindings/python`: `examples/pipeline/screen_record.py`'s header claimed "no audio
  encoder exists in the ABI" — stale; `AudioEncoder` (ABI v2) already shipped and
  `camera_record.py`'s own header already documents that gap as closed
- `bindings/cpp`: `device::ScreenCapture::pollFrame()` unconditionally threw on a
  GPU-storage frame — dead code that only surfaced once `ScreenCapture::open()` could
  actually succeed, since GPU storage is the only real case for Screen (no CPU
  fallback). Also never queried negotiated geometry (`info()` stayed `{0,0,...}`
  forever) and had no `releaseFrame()` at all. `examples/pipeline/screen_record.cpp`'s
  header carried the same stale "no audio encoder exists in the ABI" claim as Python's

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
  Intel UHD 770. GPU adapter enumeration and a configurable DirectX11 device
  factory (`mediaway-device::windows::GpuDevice`) now give every binding a way
  to construct a real GPU device without raw COM interop.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified. All 4 C-ABI bindings (C++/C#/Python/Node.js) verified
  for the container capability on Linux x64 (pure CPU); device/pipeline
  capabilities remain Windows-hardware-verified only.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`, WebCodecs encode
  and decode (`DecodeSession`); encoder/decoder/device crates build for
  wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`, with
  multi-frame GOP + CBR rate control and live `set_bitrate`), QuickSync (VPL);
  AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video, both
  hardware-verified for GPU decode; AAC — software (ADTS). Auto video
  decode C ABI (CPU output), reachable from all 4 non-Rust bindings.
- Audio: Opus — Windows decode via Media Foundation and a cross-platform
  software decoder (both behind the `AudioDecoder` trait), software
  encode/decode (`unsafe-libopus`); audio processing module (sonora); AAC —
  software encode (C# `Mediaway.Pipeline.AudioEncoder`, ABI v2). Opus decode
  C ABI reachable from all 4 non-Rust bindings.
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); `rtp-core` for RTP payloadization (H.264/HEVC).

### Bindings

- C: [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.1.6)
  + CMake/CPack archives (GitHub Release assets) — the GPU device factory
  (`mediaway_gpu_adapter_list`, `mediaway_gpu_device_create`/`_handle`/`_close`)
  now gives a non-Rust caller a real GPU device to drive Screen capture and
  GPU-input encode without any pre-existing device.
- C#: [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) packages
  on NuGet (Trusted Publishing, OIDC) — `Mediaway.Device.GpuDevice` and the
  `Mediaway.Pipeline` capture-to-encode bridge (`WriteFrameFromCameraCapture`/
  `WriteFrameFromDesktopCapture`) now hardware-verify real Screen capture.
- Python: [`mediaway`](https://pypi.org/project/mediaway/) on PyPI (Trusted
  Publishing) — `GpuDevice.list_adapters()`/`create()` and the capture-to-encode
  bridge now hardware-verify real Screen capture via `VideoCapture.open(source="screen")`.
- Node: [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi),
  [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container),
  [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device),
  [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder),
  [`@mediaway/decoder`](https://www.npmjs.com/package/@mediaway/decoder) on
  npm (OIDC Trusted Publishing) — `@mediaway/decoder` is a new package split
  out of `@mediaway/encoder`'s decode surface; `@mediaway/device`'s
  `listGpuAdapters`/`GpuDevice` and `@mediaway/encoder`'s capture-to-encode
  bridge now hardware-verify real Screen capture.
- C++: `mediaway::device::GpuDevice::listAdapters()`/`create()` and
  `EncodeSession`'s capture-to-encode bridge wired into
  `bindings/cpp/include/mediaway/`; completes GPU device factory + Screen
  capture + capture-to-encode bridge parity across every planned binding
  (C, Node.js, C#, Python, C++).
- Browser: [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser)
  (wasm, wasm-bindgen).

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines for
every backend. GPU device factory + Screen capture + capture-to-encode bridge
now reach parity across every planned binding (C, Node.js, C#, Python, C++),
each hardware-verified on real Windows GPUs; device and pipeline
(encode/decode) capabilities remain Windows-hardware-verified only, while
container mux/demux and video/Opus-audio decode also verify on Linux x64
(pure CPU). Costly paths (CPU readback, SW fallbacks) are documented at each
API (`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

## [0.1.5] - 2026-08-08

### What's new since 0.1.4

#### Added

- `mediaway-ffi`: WebM reaches the container C ABI (`mediaway_muxer_create_for_format`/`mediaway_demuxer_create_for_format`, ABI v1, `adr/container/0003-multi-format-c-abi.md`) — previously the C ABI (and every non-Rust binding) could only open MP4, even though `mediaway-container::webm` (VP8 mux/demux since v0.1.3) had no C-reachable path
- `mediaway-ffi`: Ogg and ADTS reach the container C ABI via dedicated single-stream handles (ABI v2 → v3, `adr/container/0004-ogg-adts-c-abi.md`) — neither format has track registration or `Open`/`Live` typestate, so they don't fit the generic muxer/demuxer handles WebM used
- `mediaway-ffi`: FLV reaches the container C ABI via dedicated handles (ABI v3 → v4, `adr/container/0005-flv-c-abi.md`) — mux writes tag bytes directly into a caller-supplied buffer, mirroring `flv::Muxer`'s own shape
- `mediaway-ffi`: MPEG-TS reaches the container C ABI via dedicated handles (ABI v4 → v5, `adr/container/0006-mpeg-ts-c-abi.md`), including the crate's only multi-packet demux call (`mediaway_ts_demuxer_finish`)
- `mediaway-ffi`: MP3 reaches the container C ABI via dedicated handles (ABI v5 → v6, `adr/container/0007-mp3-c-abi.md`)
- `mediaway-ffi`: WAV reaches the container C ABI (ABI v6 → v7, `adr/container/0008-wav-c-abi.md`), closing out all 8 `mediaway-container` formats
- C++, C#, Python, and Node.js bindings: all 8 `mediaway-container` formats wired end-to-end (WebM/Ogg/ADTS/FLV/MPEG-TS/MP3/WAV joining MP4), each verified against a real native dylib reusing shared byte patterns across the four bindings
- All 4 C-ABI bindings (C++/C#/Python/Node.js) verified on Linux x64, container capability only — `mediaway-ffi` needed zero Rust changes
- C++, C#, Python, and Node.js bindings: video decode (`DecodeSession`) and Opus audio decode (`AudioDecodeSession`) reach all four bindings — the decode session C ABI (`adr/0004-auto-decode-c-abi.md`, `adr/pipeline/0006-audio-decode-c-abi.md`) existed since v0.1.4 with no binding wired to it. Each binding mirrors its existing `AutoVideoEncoder`/`AudioEncoder` single-step-handle shape; the Opus round trip encodes via a raw-ABI path in each language (except Python, whose `AudioEncoder.open()` already accepted `codec=Codec.OPUS`)

#### Fixed

- `mediaway-ffi`: `container.h`'s `mediaway_codec_kind_t` was missing `MEDIAWAY_CODEC_VP8`; `mediaway_container_ffi_abi_version()` had drifted to a stale hardcoded `0`
- `mediaway-common`: `CodecKind` gains explicit `#[repr(u8)]` discriminants, found while wiring the C++ bindings
- C++/Python/Node.js bindings: `Muxer`'s auto-assigned track ids started at `0`, silently rejected by WebM/Matroska (TrackNumber must not be `0`); now start at `1`
- Node.js binding: `RawPacket`'s TypeScript interface was missing a `dts` field the underlying C ABI already had; `@mediaway/container`/`device`/`encoder`'s internal `@mediaway/*` cross-dependency pins were exact-version instead of caret, so npm's workspace linker could silently resolve stale published packages instead of local sources

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
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified. All 4 C-ABI bindings (C++/C#/Python/Node.js) now verified
  for the container capability on Linux x64 (pure CPU); device/pipeline
  capabilities remain Windows-hardware-verified only.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`, WebCodecs encode
  and decode (`DecodeSession`); encoder/decoder/device crates build for
  wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

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

### Bindings

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

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines for
every backend. Container mux/demux and video/Opus-audio decode bindings now
reach parity across four ecosystems (C/C++, C#, Python, Node.js) and two
platforms (Windows, Linux x64 for the pure-CPU container capability); device
and pipeline (encode/decode) capabilities remain Windows-hardware-verified
only. Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

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

- C: [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.1.4)
  + CMake/CPack archives (GitHub Release assets) — video decode,
  capture-to-encode bridge, and Opus audio decode/encode all newly reachable
  this release.
- C#: [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) packages
  on NuGet (Trusted Publishing, OIDC) — GOP/CBR encode config and live
  `SetBitrate` newly reachable this release.
- Python: [`mediaway`](https://pypi.org/project/mediaway/) on PyPI (Trusted
  Publishing).
- Node: [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi),
  [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container),
  [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device),
  [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder) on
  npm (OIDC Trusted Publishing).
- Browser: [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser)
  (wasm, wasm-bindgen) — now decode-capable via `DecodeSession`, not just
  encode.

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
