# Mediaway release notes

<!-- Dev changes accumulate under ## Unreleased (AGENTS.md § 10). Finalize
     with `/release-notes <version>`; reset this template with
     `/release-notes reset`. See docs/ai/wiki/meta/release-notes.md. -->

## Unreleased

### Added

- `mediaway-device`: GPU adapter enumeration (`windows::enumerate_gpu_adapters`) and a
  configurable DirectX11 device factory (`windows::GpuDevice`) — the first reusable way
  for a caller without a pre-existing GPU device to get a real `GpuDeviceHandle` for
  Zero-Copy capture/encode/decode paths.
- `mediaway-ffi`: C ABI for the GPU device factory (`mediaway_gpu_adapter_list`,
  `mediaway_gpu_device_create`/`_handle`/`_close`) — the first way for a non-Rust
  caller to create a real GPU device and drive Screen capture / GPU-input encode from
  outside Rust. `bindings/c/examples/device/capture_screen.c` and
  `bindings/c/examples/pipeline/screen_record.c` now link+run-verify real Screen
  capture on real hardware instead of only demonstrating the gap.
- `@mediaway/decoder` (Node.js): new npm package — video decode + Opus audio decode,
  split out of `@mediaway/encoder`'s previously undiscoverable `decode.ts`.
- `@mediaway/device` (Node.js): the GPU device factory (`listGpuAdapters`,
  `GpuDevice`) and real Screen capture — `openScreenCapture()` creates a GPU device
  internally (or accepts a caller-supplied one) instead of always throwing
  `CaptureUnavailableError`.
- `@mediaway/encoder` (Node.js): the capture-to-encode bridge
  (`EncodeSession.writeFrameFromCameraCapture`/`writeFrameFromDesktopCapture`) and
  `AutoVideoEncodeConfig.gpuDevice` for Zero-Copy GPU input — `examples/device/capture-screen.ts`
  and `examples/pipeline/screen-record.ts` now run-verify real Screen capture on real
  hardware instead of only demonstrating the gap.
- `Mediaway.Device` (C#): the GPU device factory (`GpuDevice.ListAdapters`/`Create`/
  `TryCreate`) — the first way for a C# caller to construct a real `ID3D11Device` without
  raw COM interop.
- `Mediaway.Pipeline` (C#): the capture-to-encode bridge
  (`EncodeSession.WriteFrameFromCameraCapture`/`WriteFrameFromDesktopCapture`) —
  `ScreenRecord.cs` now builds its GPU device via the new factory and streams through the
  bridge instead of a `NotImplementedException` placeholder, and
  `Mediaway.Device.Tests`/`Mediaway.Pipeline.Tests` hardware-verify real Screen capture and
  the bridge on real hardware instead of a hand-rolled test-only `D3D11CreateDevice`.
- `mediaway` (Python): `GpuDevice.list_adapters()`/`create()` and the capture-to-encode
  bridge (`EncodeSession.write_frame_from_camera_capture`/`write_frame_from_desktop_capture`)
  — `VideoCapture.open(source="screen")` now opens real GPU-backed Screen capture instead
  of always raising `CaptureUnsupportedError`; `examples/device/capture_screen.py` and
  `examples/pipeline/screen_record.py` now run-verify real Screen capture on real hardware
  instead of only demonstrating the gap.
- `mediaway::device::GpuDevice` (C++): `listAdapters()`/`create()` — the first way for a
  C++ caller to construct a real `ID3D11Device` without raw COM interop.
  `mediaway::device::ScreenCapture::open()` now opens real Zero-Copy Screen capture
  (`ScreenCaptureConfig::gpuDevice`) instead of always throwing `Status::Unsupported`.
- `mediaway::encoder::EncodeSession` (C++): the capture-to-encode bridge
  (`writeFrameFromCameraCapture`/`writeFrameFromDesktopCapture`) —
  `examples/device/capture_screen.cpp` and `examples/pipeline/screen_record.cpp` now
  link+run-verify real Screen capture and the bridge on real hardware instead of only
  demonstrating the gap. This completes GPU device factory + Screen capture +
  capture-to-encode bridge parity across every planned binding (C, Node.js, C#, Python, C++).

### Changed

### Fixed

- `mediaway-ffi`: `mediaway_pipeline_ffi_abi_version()` was still returning `5` while
  `include/mediaway/pipeline.h`'s `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` had already been
  bumped to `6` — every C caller's own ABI-version self-check was silently failing.
- `bindings/nodejs`: `bun install`'s default per-package/isolated workspace linking
  did not hoist `@mediaway/*` into the root `node_modules`, breaking root-level
  `tsc --noEmit`/`tsx` resolution for `test/*.ts` (pre-existing, reproduced even on
  files untouched by this change) — fixed via `bunfig.toml`'s `[install] linker = "hoisted"`.
- `bindings/csharp`: `ScreenRecord.cs`'s `DrainAudioAsync` had an ambiguous bare
  `AudioFrame` reference between `Mediaway.Device.Audio.AudioFrame` and
  `Mediaway.Pipeline.AudioFrame` (pre-existing, only surfaced once `Mediaway.Pipeline`
  referenced `Mediaway.Device.Camera`/`Mediaway.Device.Desktop` for the bridge above) —
  fixed by fully qualifying the type.
- `bindings/python`: `examples/pipeline/screen_record.py`'s header claimed "no audio
  encoder exists in the ABI" — stale; `AudioEncoder` (ABI v2) already shipped and
  `camera_record.py`'s own header already documents that gap as closed.
- `bindings/cpp`: `device::ScreenCapture::pollFrame()` unconditionally threw on a
  GPU-storage frame — dead code that only surfaced once `ScreenCapture::open()` could
  actually succeed, since GPU storage is the only real case for Screen (no CPU
  fallback). Also never queried negotiated geometry (`info()` stayed `{0,0,...}`
  forever) and had no `releaseFrame()` at all. `examples/pipeline/screen_record.cpp`'s
  header carried the same stale "no audio encoder exists in the ABI" claim as Python's.

### Removed

### Deprecated

### Breaking
