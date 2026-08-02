# ADR-0001: `adts` — freestanding ADTS (raw AAC) mux + demux

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `adts`

## Context

`iso-bmff` already has a one-off ADTS-header-strip helper
(`crates/iso-bmff/src/bitstream/aac.rs::strip_adts`) used when muxing a single AAC
frame into MP4. There is no standalone ADTS *container* — for raw `.aac` files,
HLS audio-only segments, or any pipeline that carries AAC without ISOBMFF, callers
have no Mediaway/freestanding crate to reach for.

## Decision

> New unprefixed freestanding crate `adts` (naming: ADR-0012), sans-io, no
> Mediaway or `iso-bmff` dependency.

- Scope: no-CRC ADTS header (`protection_absent = 1`), single raw-data-block per
  frame (`number_of_raw_data_blocks_in_frame = 0`) — the profile essentially every
  AAC-LC encoder emits. CRC-protected headers (9-byte) are recognized and skipped
  correctly on the *demux* side (header length adapts), but the muxer only ever
  writes the no-CRC form.
- Bit layout cross-checked field-for-field against `iso-bmff`'s existing
  `strip_adts` (`profile`/`sampling_frequency_index`/`channel_configuration` byte
  positions) so both crates agree on the wire format; `adts` does not depend on
  `iso-bmff` (freestanding, no shared code — the overlap is `strip_adts` handling a
  single already-buffered frame for MP4 muxing, while `adts` owns the full
  container mux/demux over a byte stream).
- `Muxer::write_frame` appends one self-contained frame per call — ADTS has no
  container-level header at all, so unlike `iso-bmff`'s box-based mux there is no
  `finish()` step.
- `Demuxer` is a true incremental `push_bytes`/`poll_frame` reader (matches
  `iso-bmff`'s demux shape) — unlike `riff-wave` (ADR-0001 in that crate), ADTS
  frames genuinely are independently streamable, so no buffer-everything
  compromise was needed here.
- Reserved/out-of-range `sampling_frequency_index` (13-15) and a bad sync word both
  return `Err`, never silently misparsed.

## Consequences

- No `mediaway-container` facade wiring yet (freestanding core only).
- Multi-raw-data-block ADTS frames (rare in practice) are not supported — such a
  stream would parse each ADTS header as one frame per the 13-bit length field,
  which already spans all raw blocks, so the *payload bytes* survive; the crate
  simply doesn't expose the multi-AAC-frame-per-ADTS-frame boundary.

## References

- `crates/iso-bmff/src/bitstream/aac.rs` — the pre-existing single-frame reference
- ADR-0012 (workspace) — unprefixed freestanding-core naming
- [ISO/IEC 13818-7](https://www.iso.org/standard/68280.html) (MPEG-2 Advanced Audio Coding, Annex to ADTS) — informational reference only, not pinned (implemented directly from the well-known fixed-width bitfield layout already cross-checked against `strip_adts`)
