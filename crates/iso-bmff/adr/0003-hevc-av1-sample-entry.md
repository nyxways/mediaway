# ADR-0003: HEVC (`hvc1`/`hvcC`) + AV1 (`av01`/`av1C`) sample-entry mux + demux, and honest `ftyp` brands

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-bmff`

## Context

`Codec::Hevc` and `Codec::Av1` already exist in `crate::types::Codec`, but
`sample_entry.rs`'s `write_stsd` had no branch for either — like VP9 before
ADR-0002, both fell into the `avc1`/`avcC` fallback, mislabeling HEVC/AV1
tracks as H.264. `write_ftyp` compounded this: it hardcoded `avc1` as a
compatible brand for every file regardless of track codecs, so even an
audio-only or VP9-only file falsely claimed AVC compatibility. Both gaps were
explicitly flagged as deferred, not silently dropped, in ADR-0002.

## Decision

> Add `hvc1`/`hvcC` and `av01`/`av1C` sample-entry writers and parsers,
> following the same shape as ADR-0002's `vp09`/`vpcC`. Make `write_ftyp`'s
> compatible-brand codec-aware instead of hardcoded.

- `write_stsd` gains `Codec::Hevc → write_hvc1` and `Codec::Av1 → write_av01`
  branches — structurally identical `VisualSampleEntry` header to
  `write_avc1`/`write_vp09`, with `hvcC`/`av1C` child boxes.
- Placeholder configs when `track.extra_data` (a demuxed real config payload)
  is unavailable: `HVCC_PLACEHOLDER` is a minimal valid
  `HEVCDecoderConfigurationRecord` (ISO/IEC 14496-15 — pinned paywalled entry
  `iso-14496-15`, `lengthSizeMinusOne = 3` to match this crate's 4-byte NAL
  length framing convention, zero parameter-set arrays); `AV1C_PLACEHOLDER` is
  a minimal valid `AV1CodecConfigurationRecord` (`marker=1`, `version=1`, rest
  zeroed, no `configOBUs` — pinned free entry `av1-isobmff-binding`, fetched
  and digest-verified this session). Same honesty posture as `avcC`/`vpcC`'s
  placeholder fallbacks: a real demuxed config always wins.
- `parse_sample_entry` recognizes `hvc1`/`hev1` and `av01`, capturing the
  `hvcC`/`av1C` payload the same way `avcC`/`vpcC`/`esds` are captured.
- `write_ftyp(buf, tracks: &[Track])` (signature change — one caller,
  `mux/mod.rs`) now picks the compatible brand from the first video-codec
  track present (`avc1`/`hvc1`/`av01`/`vp09`), omitting it entirely for
  audio-only files or codecs with no ISOBMFF brand yet (`Aac`/`Opus`/
  `WebVtt`/`Tx3g`) — matching `sample_entry.rs::write_stsd`'s own mapping
  instead of independently guessing.
- No bitstream-specific handling added: like VP9, HEVC/AV1 samples pass
  through `push_packet` unchanged (`mux/mod.rs`'s codec match only special-
  cases `H264`→AVCC and `Aac`→ADTS-strip) — framing correctness for HEVC/AV1
  payloads (4-byte NAL lengths, matching `lengthSizeMinusOne`) is the
  caller's responsibility, same as it already was for VP9.

## Consequences

### Positive

- HEVC and AV1 tracks now round-trip through fMP4 mux → demux with the
  correct sample entry and an honest `ftyp` (verified:
  `sample_entry_tests.rs`, `tests/roundtrip.rs::fmp4_hevc_roundtrip` /
  `fmp4_av1_roundtrip`).
- `ftyp`'s compatible brands no longer claim AVC compatibility for non-AVC
  files — closes the gap flagged in ADR-0002.

### Negative / Trade-offs

- `write_ftyp`'s signature changed (`&[Track]` param added) — a breaking
  change to this crate's public API; only one in-tree caller existed.
- Placeholder `hvcC`/`av1C` configs are structurally valid but not decodable
  as-is (no real parameter sets / sequence header) — same caveat as
  `VPCC_PLACEHOLDER`, acceptable because a real muxer always has demuxed or
  encoder-provided config data by the time it writes a track.

## References

- [ADR-0002](0002-vp9-sample-entry.md) — VP9 precedent this follows
- [`iso-14496-15`](../../../docs/standards/registry.toml) — HEVC in ISOBMFF (paywalled, not cached)
- [`av1-isobmff-binding`](../../../docs/standards/registry.toml) — AV1 ISOBMFF binding (free, fetched + pinned this session)
