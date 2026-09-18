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

- Encoder capability probe no longer reports `NoDevice` for backends that work. It opened
  every probe session at 64×64, below NVENC's minimum dimensions, so `encoder_support`
  reported no NVIDIA encoder on an RTX 4090 that encodes AV1 and H.264 fine. The default
  probe size is now one measured to clear every backend's minimum.

### Removed

### Deprecated

### Breaking
