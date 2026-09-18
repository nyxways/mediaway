# Mediaway release notes

<!-- Accumulate development changes under ## Unreleased

### Added

- `mediaway_encoder::windows::auto::support_at` and `mediaway::platform::encoder_support_at`
  — probe encoder availability at a caller-supplied resolution. Encoder support is
  resolution-dependent; the resolution-free `support`/`encoder_support` forms remain and now
  delegate at a documented default
  (`crates/mediaway-encoder/adr/0005-resolution-aware-capability-probe.md`).

### Changed

### Fixed

- **DX11 Zero-Copy encode now works on NVIDIA hardware.** Three async-MFT sequencing bugs
  made `VideoInputPreference::ZeroCopyGpu` fail on every machine that selected an async
  encoder MFT — which is every NVIDIA machine, since Intel QuickSync's MFTs are sync and
  NVIDIA's are not: the async unlock came after `MFT_MESSAGE_SET_D3D_MANAGER` (so `open`
  failed with `MF_E_TRANSFORM_ASYNC_LOCKED`), `METransformHaveOutput` was received and
  discarded (so the first `push_frame` failed), and the input wait never drained output (so
  the second `push_frame` deadlocked). Verified on an RTX 4090 at 1920×1080 for H.264 and
  HEVC, from NV12 **and** BGRA input
  (`crates/mediaway-encoder/adr/windows/0012-async-mft-zero-copy-sequencing.md`).
  This was previously recorded in the wiki as an unexplained "environment gap"; that
  diagnosis is retired.

- Encoder capability probe no longer reports `NoDevice` for backends that work. It opened
  every probe session at 64×64, below NVENC's minimum dimensions, so `encoder_support`
  reported no NVIDIA encoder on an RTX 4090 that encodes AV1 and H.264 fine. The default
  probe size is now one measured to clear every backend's minimum.

### Removed

### Deprecated

### Breaking
