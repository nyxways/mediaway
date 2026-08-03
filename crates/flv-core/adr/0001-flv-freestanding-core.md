# ADR-0001: `flv` — freestanding FLV tag mux + demux

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `flv`

## Context

FLV (Flash Video) remains common for RTMP ingest/streaming pipelines even as
native Flash playback has faded. No FLV support existed anywhere in the
workspace.

## Decision

> New unprefixed freestanding crate `flv` (naming: ADR-0012), sans-io, no
> Mediaway dependency.

- Scope: the FLV **container** structure only — file header (`FLV` signature,
  version, audio/video presence flags, `DataOffset`), tag header (`TagType`,
  `DataSize`, split 24-bit+8-bit timestamp, `StreamID`), and the trailing
  `PreviousTagSize` after every tag. `Tag::data` is an opaque already-formatted
  payload — this crate does **not** interpret or build the codec-specific
  sub-framing inside tag data (`AudioTagHeader`/`VideoTagHeader`, e.g.
  `AVCPacketType`/composition time for AVC video tags, `AACPacketType` for AAC
  audio tags). Same "frame, don't encode" boundary as `adts`/`mpeg-audio`/`ogg`.
- `Muxer` has no `finish()` — FLV tags are independently appendable, each
  self-trailed with its own `PreviousTagSize` (unlike `riff-wave`, nothing about
  FLV requires knowing the total stream length up front).
- `Muxer::write_tag` before `Muxer::write_header` is a hard `Err`
  (`Error::HeaderNotWritten`) rather than emitting a malformed file.
- `Demuxer` reads `DataOffset` from the header rather than hardcoding 9 bytes
  (the spec allows a larger header), and is a true incremental
  `push_bytes`/`poll_tag` reader, tested byte-by-byte across header/tag
  boundaries to confirm it never misparses on a short read.
- The file header's `TypeFlagsAudio`/`TypeFlagsVideo` bit positions (bit 2 / bit
  0 respectively) were confirmed against widely-observed FLV files (common
  combined flags byte `0x05` = both present) rather than a pinned spec copy —
  Adobe's FLV spec is not freely redistributable; see References.

## Consequences

- No `mediaway-container` facade wiring yet (freestanding core only).
- Script-data (AMF) tags are framed but not parsed as AMF — `Tag::data` is
  returned as opaque bytes for `TagType::ScriptData` too.

## References

- `crates/adts/adr/0001-adts-freestanding-core.md`, `crates/mpeg-audio/adr/0001-mpeg-audio-freestanding-core.md`, `crates/ogg/adr/0001-ogg-freestanding-core.md` — same "frame already-formatted data" boundary applied to sibling formats
- ADR-0012 (workspace) — unprefixed freestanding-core naming
- Adobe FLV/F4V specification — not freely redistributable; not pinned via the standards registry. Field layout implemented from widely-documented community references (e.g. multimedia.cx FLV format notes) and cross-checked via this crate's own mux↔demux round-trip tests
