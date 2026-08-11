# Mediaway v0.1.6

## What's new

### Added

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

### Fixed

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

## Codecs

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

## Bindings

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

## Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

## Maturity bar

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
