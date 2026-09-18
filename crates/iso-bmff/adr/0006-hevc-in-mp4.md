# ADR-0006: Make HEVC-in-MP4 actually decodable — unescape the SPS, length-prefix the samples

- **Status**: Accepted
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-bmff` (with a one-arm change in `mediaway-encoder`)

## Context

Every HEVC MP4 this workspace produced decoded **zero frames**. Measured, not inferred: a
file muxed from a real WMF HEVC encoder was handed to ffmpeg, which reported
`Invalid NAL unit size (17564159 > 22953)` for every sample and decoded nothing. An H.264 file
down the identical code path decoded 30 of 30 frames.

The gap had been open since ADR-0003 added the `hvc1`/`hvcC` sample entry, and three separate
decisions each contributed a necessary part of it. None of them was wrong in isolation, which
is why it survived: each deferred a piece to a caller that did not exist.

1. **`mediaway-encoder` never built an `hvcC`.** `wmf/video.rs::refresh_extradata` converted
   H.264's sequence header to `avcC` and AV1's to `av1C`, and let HEVC fall through a `_` arm
   that passed Media Foundation's raw `MF_MT_MPEG_SEQUENCE_HEADER` blob — Annex-B VPS/SPS/PPS
   — straight through as `extra_data`. `mediaway-encoder`'s ADR-0010 scoped this out
   deliberately and named it as follow-up work; this is that follow-up.

2. **`iso-bmff` wrote that blob into `hvcC` verbatim**, and when `extra_data` was empty (the
   inbox `HEVCVideoExtensionEncoder` MFT never sets the attribute at all) it wrote
   `HVCC_PLACEHOLDER`, whose `numOfArrays` is 0 — a record with no parameter sets. ADR-0003
   called the placeholder "structurally valid but not decodable as-is," which was accurate and
   turned out to be the shipping path rather than a fallback.

3. **`Muxer::push_packet` did not length-prefix HEVC samples.** ADR-0003 recorded this as
   "the caller is responsible for correct NAL-length framing (unlike `Codec::H264`, which gets
   automatic Annex-B → AVCC conversion)." In practice no caller did it, and none could
   reasonably be expected to: every encoder in this workspace emits Annex-B, and `hvcC`
   declares `lengthSizeMinusOne = 3`. A start code `00 00 00 01` read as a length is a 1-byte
   NAL followed by garbage — exactly the ffmpeg error above.

A fourth defect was found while fixing the first three and is the most instructive:
**`build_hvcc` read `profile_tier_level()` at fixed byte offsets into the NAL as transmitted.**
Its doc comment justified this by analogy to `build_avcc`. The analogy does not hold. H.264's
`profile_idc`/`constraint_flags`/`level_idc` sit at RBSP bytes 1..4, where an
emulation-prevention escape cannot occur — it needs two preceding zero bytes and byte 0 is a
non-zero NAL header. HEVC's `profile_tier_level()` spans RBSP bytes 3..15 and is zero-heavy; a
Main-profile SPS from either NVENC or x265 carries **three** `00 00 03` escapes inside exactly
that range. Measured against ffmpeg's `hvcC` for the same encoder and the same sequence
header, **6 of the 23 header bytes were wrong**, including `general_level_idc`, which read
`0x00` instead of `0x5a` — a record claiming "level unspecified" for a level-3.0 stream.

That defect is **not Windows-specific**. `crates/mediaway-encoder/src/apple/videotoolbox/
extradata.rs` already called `to_hvcc`, so every HEVC file this workspace produced on macOS
carried the same corrupted header.

### Why the test suite did not catch any of it

- The HEVC mux test fed the muxer six hand-written bytes (`[0,0,0,2,0x26,0x01]`) and asserted
  the box type. Synthetic payloads cannot exhibit escaping, framing, or decodability.
- `build_hvcc`'s unit test used an SPS fixture with no `00 00 03` sequence in it, so it
  asserted the offsets and nothing about unescaping — and passed either way.
- The only encode→mux→demux integration tests were H.264-only **and** orphaned: they have no
  `[[test]]` entry in `Cargo.toml` and import a crate that no longer exists, so cargo never
  built them.
- `mediaway-encoder`'s ADR-0012 states HEVC was "verified on an RTX 4090." That verification
  asserted the encoder produced packets. It says nothing about the container, and it reads in
  `README.md` as an end-to-end claim.

## Decision

> Make Annex-B → length-prefixed conversion and configuration-record construction the
> **muxer's** job for HEVC, exactly as it already is for H.264, and read `profile_tier_level()`
> out of the unescaped RBSP.

Four changes:

1. `bitstream/hevc.rs::build_hvcc` unescapes the SPS before reading its header fields. The
   parameter-set arrays keep the NAL **as transmitted**, which is what the record is specified
   to carry — unescaping those would corrupt the parameter sets a decoder reads back out.
2. `build_hvcc` reads `numTemporalLayers` and `temporalIdNested` from SPS RBSP byte 2 rather
   than hardcoding `1`/`0`. That byte precedes `profile_tier_level()`, so no escape can reach
   it. `constantFrameRate` stays `0` ("unknown") — claiming otherwise would be a statement
   about the stream this function cannot check.
3. `mux/mod.rs::push_packet` gains a `Codec::Hevc` arm calling `to_hvcc`, which length-prefixes
   the samples and backfills `hvcC` from the first packet's parameter sets. The backfill only
   fires while `extra_data` is empty, so a real record supplied at registration still wins.
4. `mediaway-encoder`'s `wmf/video.rs::refresh_extradata` gains a `CodecKind::Hevc` arm calling
   `to_hvcc`. This is now belt-and-braces rather than load-bearing — change 3 covers the MFT
   that publishes no blob at all — but it is what puts a correct record in `StreamInfo` for a
   caller that never reaches a muxer.

### Rejected: keep framing as the caller's responsibility, document it harder

ADR-0003's position, and the reason this shipped broken for seven weeks. A contract that every
in-tree caller violates, that no test checks, and whose violation produces a valid-looking file
is not a contract — it is a trap. H.264 has had automatic conversion since the beginning and
nobody has argued it should not.

### Rejected: parse the full SPS with an exp-golomb reader

It would let `chroma_format_idc`, `bit_depth_*` and `min_spatial_segmentation_idc` be real
rather than defaulted at 4:2:0/8-bit. Not done here: those fields are informational, no decoder
in the test matrix rejects the defaults, and a bitstream parser is a much larger surface to get
wrong than the 12 bytes this change actually needed. `to_av1c` has the same posture. Revisit
when a real stream is found that needs it, not before.

## Consequences

- **HEVC MP4 files this workspace writes now decode.** Verified end to end: 30 of 30 frames,
  ffprobe, on a file muxed with **no** out-of-band configuration record.
- **macOS HEVC output is fixed too**, incidentally — the Apple backend shares `to_hvcc`.
- `hvc1` is still the box type written while samples carry in-band parameter sets, which
  `hev1` is the strictly correct brand for. Unchanged from ADR-0003, and matching what
  `to_avcc` does for H.264: every decoder in the matrix accepts it. Flagged, not fixed.
- **Verification is now a test, not a claim.** `tests/conformance_hevc.rs` encodes real HEVC
  with ffmpeg, strips it to Annex-B, muxes it through this crate with an empty `extra_data`,
  and asserts ffprobe decodes every frame. It skips loudly when ffmpeg/ffprobe/an HEVC encoder
  is missing. Both regressions were confirmed to fail it individually before the fix landed.
- `build_hvcc`'s escape-free unit test is kept, renamed to say what it actually covers, and
  paired with one built from a real NVIDIA MFT sequence header whose expected bytes are
  cross-checked against ffmpeg.
- **`README.md`'s Windows HEVC mark and `RELEASE_NOTES.md`'s HEVC claim were corrected** in the
  same change. They described an encoder-level verification in language that read as end to
  end, which is how a defect this size stayed invisible.
