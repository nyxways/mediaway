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

- `mediaway-ffi`: Ogg and ADTS reach the container C ABI via dedicated single-stream handles
  (`mediaway_ogg_muxer_t`/`_demuxer_t`, `mediaway_adts_muxer_t`/`_demuxer_t`, ABI v2 → v3,
  `adr/container/0004-ogg-adts-c-abi.md`) — neither format has track registration or
  `Open`/`Live` typestate, so they don't fit the generic `mediaway_muxer_t`/
  `mediaway_demuxer_t` handles WebM used. FLV/MPEG-TS/MP3/WAV remain Rust-only; no
  language-binding wiring yet. Verified end-to-end:
  `tests/ogg_adts_container_smoke.rs` round-trips a real `OpusHead` header + Opus packet and
  two raw AAC frames.
- `mediaway-ffi`: FLV reaches the container C ABI via dedicated `mediaway_flv_muxer_t`/
  `_demuxer_t` handles (ABI v3 → v4, `adr/container/0005-flv-c-abi.md`) — its mux side
  writes tag bytes directly into a caller-supplied buffer on every call (no `poll_bytes`
  step) with a fixed one-video/one-audio track slot, mirroring `flv::Muxer`'s own Rust
  shape. MPEG-TS/MP3/WAV remain Rust-only; no language-binding wiring yet. Verified
  end-to-end: `tests/flv_container_smoke.rs` round-trips one AVC video packet and one AAC
  audio packet, plus unsupported-codec/unregistered-stream rejection.
- `mediaway-ffi`: MPEG-TS reaches the container C ABI via dedicated `mediaway_ts_muxer_t`/
  `_demuxer_t` handles (ABI v4 → v5, `adr/container/0006-mpeg-ts-c-abi.md`) — elementary
  streams are registered at muxer construction (no `add_track`), `write_pat_pmt`/
  `write_access_unit` write directly into a caller-supplied buffer with explicit
  `pts_90k`/`dts_90k` clock values (not a track-timebase packet), and
  `mediaway_ts_demuxer_finish` returns an owned array of packets — the only multi-packet
  demux call in this crate, with its own `mediaway_ts_demuxer_finish_free`. MP3/WAV remain
  Rust-only; no language-binding wiring yet. Verified end-to-end:
  `tests/ts_container_smoke.rs` round-trips one H.264 video and one AAC audio access unit,
  a `finish()`-recovered trailing access unit, and an invalid-PID construction rejection.
- `mediaway-ffi`: MP3 (MPEG Layer III) reaches the container C ABI via dedicated
  `mediaway_mp3_muxer_t`/`_demuxer_t` handles (ABI v5 → v6, `adr/container/0007-mp3-c-abi.md`)
  — a fixed header for the mux session's lifetime (no track registration at all) and an
  explicit `padding` bit on `write_frame` that no `mediaway_packet_view_t` has a slot for.
  WAV remains Rust-only; no language-binding wiring yet. Verified end-to-end:
  `tests/mp3_container_smoke.rs` round-trips a 128 kbps/44100 Hz stereo frame, a mono
  channel-count case, and a wrong-frame-body-length rejection.
- `mediaway-ffi`: WAV (RIFF/WAVE PCM) reaches the container C ABI (ABI v6 → v7,
  `adr/container/0008-wav-c-abi.md`) — closing out all 8 `mediaway-container` formats.
  `mediaway_wav_muxer_t` is mux-only, since `wav::Muxer::finish` consumes `self` by value
  (RIFF chunk sizes must be known up front); demux has no handle at all —
  `mediaway_wav_parse` is a one-shot whole-buffer function, unlike every other format in
  this crate. No language-binding wiring yet for any of the 6 non-MP4/WebM formats. Verified
  end-to-end: `tests/wav_container_smoke.rs` round-trips PCM and float-format frames, a
  double-`finish()` rejection, and a non-RIFF/WAVE-data rejection.
- C++ binding: all 8 `mediaway-container` formats reach `bindings/cpp/include/mediaway/`
  (WebM via `container::Muxer`/`Demuxer(Format::Webm)`; Ogg/ADTS/FLV/MPEG-TS/MP3 get
  dedicated classes; WAV is mux-only via `WavMuxer` + the one-shot `wavParse()` function).
  Split the single 892-line `mediaway.hpp` into `core.hpp`/`container.hpp` (+ per-format
  headers under `container/`)/`pipeline.hpp`/`device.hpp` to stay under the workspace's
  1000-line source cap; `mediaway.hpp` remains a pure umbrella include. C#/Python/Node
  wiring still pending. Verified end-to-end: `examples/container/all_formats_smoke.cpp`
  links and runs against the real GNU-target dylib.

### Changed

### Fixed

- `mediaway-ffi`: the hand-written `container.h` header's `mediaway_codec_kind_t` was
  missing `MEDIAWAY_CODEC_VP8` — the Rust-side enum has had it since v0.1.3's WebM VP8 work,
  but no C caller could ever name it. Added (`= 12`, matching the existing Rust discriminant).
- `mediaway-ffi`: `mediaway_container_ffi_abi_version()` had drifted to a stale hardcoded
  `0` since the WebM C ABI landed (the header macro had already moved to `1`) — fixed to
  track the real value (`7`, alongside this release's own bumps).
- `mediaway-common`: `CodecKind` had no explicit `#[repr(u8)]`/discriminants at all, found
  while wiring the C++ container bindings — harmless at the FFI boundary itself (the
  `mediaway-ffi`-local mirror enum already had correct explicit values and converts by
  name), but a real latent footgun now pinned explicitly to match the C header's values.
- C++ binding: `container::Muxer`'s auto-assigned track ids started at `0`, silently
  rejected by WebM/Matroska (TrackNumber must not be `0`) though harmless for MP4 — now
  start at `1` for both formats. Only reproducible by linking and running against the real
  dylib, not by `-fsyntax-only` alone.

### Removed

### Deprecated

### Breaking
