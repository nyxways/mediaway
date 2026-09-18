# ADR-0005: `Opus`/`dOps` sample entry

- **Status**: Accepted
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: iso-bmff

## Context

`Codec::Opus` has existed in this crate's `Codec` enum since it was written, and is handled
as audio in `codec_features.rs`, `ftyp.rs` and `moov.rs`. It was **not** handled in
`sample_entry.rs`:

```rust
let audio = matches!(track.codec, Codec::Aac | Codec::Opus);
if audio { write_mp4a(st, track); }
```

`write_mp4a` writes an `mp4a` sample entry containing an `esds` whose bytes are
unconditionally `0x40, 0x15` — `objectTypeIndication` 0x40 (MPEG-4 Audio, i.e. AAC) and
`streamType` 0x15 (audio) — followed by an `AudioSpecificConfig` defaulting to `[0x12, 0x10]`
(AAC-LC, 44.1 kHz, stereo).

So muxing an Opus track produced **a file declaring AAC over Opus packets**, with a
hardcoded AAC configuration record. It muxes without error and fails at playback, which is
the worst shape a container bug can take: nothing in the writing path can notice, and the
reader blames the wrong component.

This was found while deciding qarec's audio codec, not by a test — nothing exercised
`Codec::Opus` through `write_stsd`.

## Decision

> Opus gets its own sample entry and `OpusSpecificBox`, per
> [Encapsulation of Opus in ISOBMFF][opus-isobmff]. `write_mp4a` is now reached only by
> `Codec::Aac`.

Three field values are fixed by that spec rather than taken from `Track`, and are worth
naming because two of them look like bugs otherwise:

| Field | Value | Why not from `Track` |
|---|---|---|
| `samplerate` | always `48000 << 16` | Opus always decodes at 48 kHz. The original rate is informational and lives in `dOps`'s `InputSampleRate`. |
| `samplesize` | always `16` | Spec-mandated. |
| `esds` | **forbidden** | `dOps` replaces it. |

### `extra_data` is an `OpusHead`, in both directions

`dOps` and RFC 7845's `OpusHead` carry **identical fields in identical order**, differing
only in byte order (big- vs little-endian) and `OpusHead`'s 8-byte magic. So the conversion
is a re-serialization, not a parse, in both `opus_head_to_dops` and `dops_to_opus_head`.

Demux hands back an `OpusHead` rather than the raw `dOps` payload. That is deliberate and
differs from how video works here — `vpcC`/`hvcC`/`av1C` payloads are captured verbatim —
but it matches the **audio** precedent already in this file: `mp4a` demux yields the raw
`AudioSpecificConfig`, not the `esds` that wrapped it. It also means `extra_data` denotes
the same thing across containers, since `mediaway-container::webm` stores `OpusHead` as
`CodecPrivate`, so an MP4 → WebM remux preserves the configuration instead of moving a
container-specific box between containers.

### Placeholder when there is no real `OpusHead`

`DOPS_PLACEHOLDER` — version 0, stereo, pre-skip 3840 (RFC 7845 § 4.1's recommendation of
80 ms at 48 kHz when the encoder's own value is unknown), 48 kHz, unity gain, mapping family
0. Same posture as `VPCC_PLACEHOLDER` (ADR-0002) and `HVCC_PLACEHOLDER`/`AV1C_PLACEHOLDER`
(ADR-0003): structurally valid so the file parses, and **a documented signaling gap rather
than a silent lie**. The real channel count and pre-skip are properties of the encoder that
produced the packets; deriving them would mean inspecting the Opus bitstream, which is
outside this crate's sans-io framing boundary.

A truncated or non-`OpusHead` `extra_data` falls back to the placeholder rather than writing
a short `dOps`.

## Consequences

- Opus in MP4 is now writable and readable correctly. It was previously *claimed* to work
  (the enum variant, the audio branches) and did not.
- **This changes the bytes produced for existing `Codec::Opus` callers.** Any file written
  by a previous version with an Opus track is mislabeled and cannot be fixed by a reader;
  it has to be remuxed. There is no compatibility path worth preserving here — the old
  output was invalid.
- `mediaway-container::mp4` gains Opus support for free; no facade change was needed.
- The mapping-table tail (`ChannelMappingFamily != 0`) is carried in both directions. This
  got its own test because dropping it yields a `dOps` that parses and decodes to the wrong
  channel layout — silent corruption of the same family as the bug this ADR fixes. That
  test is also what caught an off-by-one in `dops_to_opus_head`: `ChannelMappingFamily` is
  at offset 10, not 8. Every other test passed with the wrong offset because all the fields
  involved were zero.
- Not done: no Opus-specific handling in `ftyp.rs`. `video_codec_brand` still returns `None`
  for audio codecs, which is correct — Opus has no ISOBMFF brand of its own.

## References

- [Encapsulation of Opus in ISO Base Media File Format][opus-isobmff] — registry id
  `opus-in-isobmff`. §4.3.2 (sample entry), §4.3.2 (`OpusSpecificBox`).
- [RFC 7845, Ogg Encapsulation for Opus][rfc7845] — registry id `rfc-7845-opus-ogg`. §5.1
  (`OpusHead`), §4.1 (pre-skip).

[opus-isobmff]: https://opus-codec.org/docs/opus_in_isobmff.html
[rfc7845]: https://www.rfc-editor.org/rfc/rfc7845
