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

### Changed

### Fixed

- `mediaway-ffi`: `mediaway_pipeline_ffi_abi_version()` was still returning `5` while
  `include/mediaway/pipeline.h`'s `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` had already been
  bumped to `6` — every C caller's own ABI-version self-check was silently failing.
- `bindings/nodejs`: `bun install`'s default per-package/isolated workspace linking
  did not hoist `@mediaway/*` into the root `node_modules`, breaking root-level
  `tsc --noEmit`/`tsx` resolution for `test/*.ts` (pre-existing, reproduced even on
  files untouched by this change) — fixed via `bunfig.toml`'s `[install] linker = "hoisted"`.

### Removed

### Deprecated

### Breaking
