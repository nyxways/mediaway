# Mediaway release notes

<!-- Dev changes accumulate under ## Unreleased (AGENTS.md § 10). Finalize
     with `/release-notes <version>`; reset this template with
     `/release-notes reset`. See docs/ai/wiki/meta/release-notes.md. -->

## Unreleased

### Added

- `mediaway-ffi`: WebM reaches the container C ABI (`mediaway_muxer_create_for_format`/
  `mediaway_demuxer_create_for_format`, ABI v1, `adr/container/0003-multi-format-c-abi.md`)
  — previously the C ABI (and every non-Rust binding) could only open MP4, even though
  `mediaway-container::webm` (real VP8 mux/demux since v0.1.3) has existed with no C-reachable
  path. `MuxerState`/`DemuxerState` become per-format enums; `MediawayStatus` gains
  `UnsupportedCodec`/`UnknownStream`. Ogg/ADTS/FLV/MPEG-TS/MP3/WAV are explicitly scoped out
  this pass — each needs a genuinely different, non-generic C ABI shape (see the ADR's
  Deferred section for the reasoning per format). No language-binding wiring yet (C ABI +
  header only). Verified end-to-end: `tests/webm_container_smoke.rs` round-trips 5 VP8
  frames through the new functions.

### Changed

### Fixed

- `mediaway-ffi`: the hand-written `container.h` header's `mediaway_codec_kind_t` was
  missing `MEDIAWAY_CODEC_VP8` — the Rust-side enum has had it since v0.1.3's WebM VP8 work,
  but no C caller could ever name it. Added (`= 12`, matching the existing Rust discriminant).

### Removed

### Deprecated

### Breaking
