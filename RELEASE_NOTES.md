# Mediaway release notes

<!-- Accumulate development changes under ## Unreleased below. See
     docs/ai/wiki/meta/release-notes.md and AGENTS.md § 10 (Release notes).
     Finalize with /release-notes <version>; reset with /release-notes reset. -->

## Unreleased

### Added

- `mediaway-sw`: cargo features `av1` / `h264` / `opus` (default on, unchanged behavior) so an Opus-only consumer no longer builds `rav1e`; allocation-free low-level Opus (`encode_float_into`, `decode_float_into` with PLC / in-band-FEC recovery, DTX / complexity / signal setters) and APM (`process_capture_block`, `push_render_block`, `statistics`) / VAD (`analyze_block`) entry points.
- `mediaway-device`: cargo features `audio` / `camera` / `desktop` (default on) that trim the Linux native dependencies (`pipewire`, `v4l`, `ashpd`) and the Windows camera / desktop backends; a disabled family reports `NotImplemented`.

### Changed

### Fixed

### Removed

### Deprecated

### Breaking
