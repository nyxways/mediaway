# ADR-0001: `mpeg-audio` — freestanding MPEG-1/2/2.5 Layer III ("MP3") mux + demux

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mpeg-audio`

## Context

MP3 (colloquially) is MPEG-1/2/2.5 Layer III — one of the most widely deployed
audio elementary-stream formats, with no framing support anywhere in this
workspace. Like `adts`, it needs no Mediaway-specific typing.

## Decision

> New unprefixed freestanding crate `mpeg-audio` (naming: ADR-0012), sans-io, no
> Mediaway dependency.

- Scope: **Layer III only** (`layer` field `01`). Layer I/II are structurally
  different (different bitrate tables, different frame-length coefficient, no
  practical modern usage) — recognizing them as "valid but unsupported" via
  `Error::UnsupportedLayer` rather than silently misparsing was judged the right
  scope boundary for a first version; not a corner cut, a deliberate one (simplicity
  first — "MP3" in the colloquial/commonly-used sense is Layer III).
- All three MPEG versions (1, 2, 2.5) are supported, since real-world low-bitrate
  MP3 commonly uses MPEG-2/2.5 Layer III (half/quarter sample rates) — narrowing to
  MPEG-1 only would have missed a genuinely common case.
- The crate **frames already-encoded Layer III data** — it does not encode PCM
  into Layer III bitstreams (out of scope for a container crate, same boundary as
  `adts`). `Muxer::write_frame` validates `frame_body.len()` against the exact byte
  count the header's bitrate/sample-rate/padding combination requires
  (`Error::FrameBodyLengthMismatch`), so a caller can't silently write a frame that
  would desync a real decoder.
- Padding is a **per-call** parameter (`write_frame(body, padding, out)`), not part
  of the fixed `FrameHeader` — real encoders flip it per frame for bit-reservoir
  accounting; baking it into a fixed mux-session config would have been wrong.
- `Demuxer` assumes frame-aligned input (no ID3-tag/leading-garbage resync scan) —
  matches `adts`'s "hard `Err` on bad sync, no silent scanning" posture.
- Frame-length formula cross-checked against the well-known reference value
  (MPEG-1 Layer III, 128 kbps, 44100 Hz, no padding → 417 bytes) in
  `mux_tests.rs::frame_len_matches_known_reference_value`.

## Consequences

- Layer I/II streams are rejected outright, not silently misinterpreted as Layer
  III — a real (intentional) format-coverage gap, tracked in `docs/roadmap.md`.
- No `mediaway-container` facade wiring yet (freestanding core only).
- No CRC support on the mux side (demux reads both CRC/no-CRC headers, matching `adts`'s asymmetry for the same reason).

## References

- `crates/adts/adr/0001-adts-freestanding-core.md` — same framing-only design
  philosophy, applied to a sibling elementary-stream format
- ADR-0012 (workspace) — unprefixed freestanding-core naming
- [ISO/IEC 11172-3](https://www.iso.org/standard/22412.html) (MPEG-1 Audio), [ISO/IEC 13818-3](https://www.iso.org/standard/26797.html) (MPEG-2 Audio) — informational reference only, not pinned (implemented from the well-known fixed bitrate/sample-rate tables and frame-length formula, cross-checked against the standard reference value above)
