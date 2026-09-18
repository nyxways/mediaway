# Opus in MP4 (`Opus`/`dOps`)

Crate-local [ADR-0005](../../../crates/iso-bmff/adr/0005-opus-sample-entry.md). Specs:
`opus-in-isobmff` and `rfc-7845-opus-ogg` in
[`docs/standards/registry.toml`](../../../docs/standards/registry.toml).

**`Codec::Opus` previously shared AAC's `mp4a`/`esds` branch**, so an Opus track was written
with an `esds` declaring `objectTypeIndication` 0x40 (MPEG-4 AAC) and a hardcoded AAC
`AudioSpecificConfig`. It muxed cleanly and failed on playback. Nothing exercised
`Codec::Opus` through `write_stsd`, which is why it survived.

- Spec-fixed fields, not taken from `Track`: `samplerate` is always `48000 << 16` (Opus
  always decodes at 48 kHz; the original rate is informational, inside `dOps`), `samplesize`
  is always 16, and `esds` is **forbidden** — `dOps` replaces it.
- `dOps` and RFC 7845's `OpusHead` are the same fields in the same order, differing only in
  byte order and `OpusHead`'s 8-byte magic, so the conversion is a re-serialization.
- **Demux yields an `OpusHead`, not the raw `dOps`.** Differs from video (`vpcC`/`hvcC`/`av1C`
  are captured verbatim) but matches the audio precedent — `mp4a` demux yields the raw
  `AudioSpecificConfig`, not the `esds` — and makes `extra_data` mean one thing across
  containers, since `ebml-webm` stores `OpusHead` as `CodecPrivate`.
- `DOPS_PLACEHOLDER` when `extra_data` is absent or truncated, same posture as VP9's.
- `dOps` offsets are easy to get wrong silently: `ChannelMappingFamily` is at **10**, not 8
  (`OutputGain` occupies 8..10). An off-by-one there passed every test whose fields were all
  zero; only the multichannel mapping-table test caught it.

