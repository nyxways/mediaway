# MP4 sample-entry codec coverage (`iso-bmff`)

`crates/iso-bmff/src/isobmff/sample_entry.rs` — `write_stsd` / `parse_sample_entry`.

## Coverage

| Codec | Mux sample entry | Demux recognizes |
|-------|-------------------|-------------------|
| H.264 | `avc1`/`avcC` | `avc1`/`avc3` |
| VP9 | `vp09`/`vpcC` | `vp09` |
| HEVC | `hvc1`/`hvcC` | `hvc1`/`hev1` |
| AV1 | `av01`/`av1C` | `av01` |
| AAC | `mp4a`/`esds` | `mp4a` |
| Opus | `Opus`/`dOps` | `Opus` | (see [opus-in-mp4](opus-in-mp4.md)) |

Every video `Codec` variant now has a real, correctly-labeled sample entry —
`write_stsd`'s fallback to `write_avc1` only fires for `Codec::H264` itself.
`write_ftyp`'s compatible brand matches whichever video codec is actually
present (or is omitted for audio-only files) instead of hardcoding `avc1`.

## VP9 (`vp09`/`vpcC`) — added 2026-07-29

Crate-local [ADR-0002](../../../crates/iso-bmff/adr/0002-vp9-sample-entry.md). Spec:
pinned in [`docs/standards/registry.toml`](../../../docs/standards/registry.toml) id
`vp-codec-iso-media-binding`.

- `write_vp09` mirrors `write_avc1`'s `VisualSampleEntry` header shape; only the
  child box (`vpcC` vs `avcC`) and its content differ.
- VP9 has no SPS/PPS-equivalent out-of-band config, unlike AVC — `write_vp09` reuses
  `track.extra_data` (a demuxed `vpcC` payload) when present, else writes a
  conservative placeholder (`VPCC_PLACEHOLDER`: profile 0, level 1.0, 8-bit 4:2:0,
  unspecified colour metadata). This is a real signaling gap when muxing a track
  that was never demuxed from a real `vpcC` (profile/level aren't derived from the
  actual VP9 bitstream) — same honesty posture as `avc1`'s hardcoded SPS/PPS
  fallback when `track.extra_data` is empty.
- `parse_sample_entry` captures the `vpcC` payload into `extra` the same way
  `avcC`/`esds` are captured, so demux → mux re-encode preserves the real config.

## HEVC (`hvc1`/`hvcC`) + AV1 (`av01`/`av1C`) — added 2026-07-29

Crate-local [ADR-0003](../../../crates/iso-bmff/adr/0003-hevc-av1-sample-entry.md).
Specs: `iso-14496-15` (HEVC in ISOBMFF, paywalled, not cached) and
`av1-isobmff-binding` (AOMedia, free — fetched + BLAKE3-pinned this session),
both in [`docs/standards/registry.toml`](../../../docs/standards/registry.toml).

- `write_hvc1`/`write_av01` mirror `write_vp09`'s `VisualSampleEntry` shape;
  only the child box (`hvcC`/`av1C`) differs. Only the `hvc1` box type is
  written (out-of-band parameter sets); `hev1` is recognized on parse but not
  produced.
- Same "no real out-of-band config → placeholder" posture as VP9 — but for HEVC
  the placeholder is now near-unreachable, since the backfill above supplies a
  real record from the first packet. It had been the *shipping* path for the
  inbox `HEVCVideoExtensionEncoder` MFT, which publishes no config at all:
  `HVCC_PLACEHOLDER` (minimal valid `HEVCDecoderConfigurationRecord`,
  `lengthSizeMinusOne=3` to match this crate's 4-byte NAL length framing) and
  `AV1C_PLACEHOLDER` (minimal valid `AV1CodecConfigurationRecord`, no
  `configOBUs`) are used only when `track.extra_data` (a demuxed real config)
  is empty.
- **HEVC no longer passes through unconverted** (2026-09-18,
  [ADR-0006](../../../crates/iso-bmff/adr/0006-hevc-in-mp4.md)). `push_packet`
  runs `to_hvcc` on `Codec::Hevc` exactly as it runs `to_avcc` on `Codec::H264`:
  Annex-B → 4-byte length prefixes, plus `hvcC` backfilled from the first
  packet's parameter sets. The old "the caller is responsible for NAL-length
  framing" contract was violated by every caller in the workspace and produced
  files that decoded **zero** frames. AV1/VP9 still pass through — correctly,
  since neither builds its config record out of the bitstream.
- `write_ftyp` gained a `tracks: &[Track]` parameter (breaking change, one
  in-tree caller) so the compatible brand reflects what `write_stsd` actually
  wrote instead of a separate, independently-hardcoded guess.

## Test coverage

- **End-to-end oracle:** `crates/iso-bmff/tests/conformance_hevc.rs` — real HEVC
  from ffmpeg, stripped to Annex-B, muxed here with an *empty* config record,
  then `ffprobe -count_frames` must decode all 30. A synthetic fixture cannot
  replace this: the HEVC bug lived in escaping, framing and decodability at
  once, and hand-written payloads exhibit none of the three.
- Unit: `crates/iso-bmff/src/isobmff/sample_entry_tests.rs` — write/parse round trip,
  `avc1` vs `vp09` tag selection, demuxed-`vpcC`-payload reuse.
- Integration: `crates/iso-bmff/tests/roundtrip.rs::fmp4_vp9_roundtrip` — full
  `Muxer`/`Demuxer` fMP4 round trip.
- Browser (real wasm, no WebCodecs dependency):
  `tools/e2e-web/tests/wasm-mux-roundtrip.spec.ts` "vp09 sample entry" test, via
  `crates/iso-bmff-wasm`'s `wasm_mux_vp9_bytes`/`wasm_mux_vp9_demux_smoke` — proves the
  container-level round trip Playwright/Chromium-side, decoupled from whether this
  browser build's WebCodecs actually supports VP9 (`decode-trim-splice.spec.ts`
  proves the WebCodecs-level encode/decode/trim/splice leg separately, with no
  container in between — see [decode/web-video-decode](../decode/web-video-decode.md)).

## Out of scope (left as-is)

- `hev1` (in-band parameter sets) is recognized on parse but never written —
  only `hvc1` is produced.
- Dolby Vision (`dvhe`/`dvh1`) and other HEVC/AV1 profile-specific brands.
- No bitstream validation that a track's payload actually matches its claimed
  NAL-length size / OBU framing — `iso-bmff` frames, it doesn't inspect codec
  bitstream internals (sans-io boundary).
