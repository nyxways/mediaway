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

### Changed

### Fixed

### Removed

### Deprecated

### Breaking
