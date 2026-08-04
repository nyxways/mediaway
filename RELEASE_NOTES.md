# Mediaway release notes

<!--
Development changes accumulate under "Unreleased" below. Agents add
release-note-worthy entries directly (AGENTS.md § Behavioral guidelines);
finalize a release with `/release-notes <version>`; versioned history lives
in CHANGELOG.md.
-->

## Unreleased

### Added

- Windows Opus decode via Media Foundation (public API) and software Opus encode in the facade

### Changed

- All 8 container formats (audio included) verified playable in mpv via the playback-verification example
- `flv` and `ogg` freestanding cores renamed to `flv-core` / `ogg-core` (crates.io name collisions)
- `ebml-webm` 0.2.0 — CodecPrivate API

### Fixed

- MP4s that failed to play: malformed `stsz` box and raw SPS written as `avcC`
- Playback timing corrections: ISO-BMFF mux duration/DTS delta (ADR-0004) and Ogg demux/CRC fixes

### Removed

### Deprecated

### Breaking
