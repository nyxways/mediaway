# ADR-0002: VP9 (`vp09`/`vpcC`) sample-entry mux + demux

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-bmff`

## Context

`Codec::Vp9` already exists in `crate::types::Codec` and is accepted by mux/demux
elsewhere, but `sample_entry.rs`'s `write_stsd` branched only audio-vs-not: every
non-audio codec (H.264, HEVC, AV1, VP9) got an `avc1`/`avcC` sample entry. Muxing a
VP9 track produced a container that mislabeled the codec as H.264 — dishonest, and
it silently blocked a real fMP4 container round trip for VP9 (previously flagged in
`docs/ai/wiki/decode/web-video-decode.md` § "No fMP4 mux/demux in the browser E2E").
`parse_sample_entry` had the matching gap on the read side (no `vp09` box tag
recognized).

## Decision

> Add a real `vp09`/`vpcC` sample-entry writer and parser, following the
> `VP Codec ISO Media File Format Binding` (pinned:
> [`docs/standards/registry.toml`](../../../docs/standards/registry.toml) id
> `vp-codec-iso-media-binding`).

- `write_stsd` branches `Codec::Vp9` to a new `write_vp09`, structurally identical to
  `write_avc1`'s `VisualSampleEntry` header, with a `vpcC` child box instead of `avcC`.
- Unlike AVC, VP9 has no out-of-band SPS/PPS equivalent to fall back to when a track
  has no real config: `write_vp09` reuses `track.extra_data` (a demuxed `vpcC`
  payload) when present, else writes a conservative placeholder (profile 0, level
  1.0, 8-bit 4:2:0, unspecified colour metadata, no codec-initialization data) —
  documented inline as `VPCC_PLACEHOLDER`, same honesty posture as `avcC`'s
  hardcoded fallback bytes.
- `parse_sample_entry` recognizes `vp09` and captures the `vpcC` payload into `extra`
  the same way `avcC`/`esds` are captured, so a demux → mux re-encode keeps the real
  config.
- Scope: VP9 only (matches the concrete, already-flagged gap). HEVC/AV1 sample
  entries remain unimplemented and still fall back to `avc1` — a separate,
  not-yet-committed follow-up, not silently expanded here.

## Consequences

- VP9 tracks now round-trip through fMP4 mux → demux with the correct sample entry
  (verified: `crates/iso-bmff/src/isobmff/sample_entry_tests.rs`,
  `crates/iso-bmff/tests/roundtrip.rs::fmp4_vp9_roundtrip`).
- `write_ftyp`'s compatible-brands list hardcoded `avc1` regardless of track
  codecs, and HEVC/AV1 tracks silently got an `avc1` sample entry — both were
  left out of scope for this ADR, tracked as known gaps. **Closed 2026-07-29**:
  see `adr/0003-hevc-av1-sample-entry.md`.

## References

- [`vp-codec-iso-media-binding`](../../../docs/standards/registry.toml) — pinned spec
- [`docs/ai/wiki/decode/web-video-decode.md`](../../../docs/ai/wiki/decode/web-video-decode.md) § "No fMP4 mux/demux in the browser E2E"
- ADR-0001 (this crate) — `video` feature gates `Codec::Vp9` registration
