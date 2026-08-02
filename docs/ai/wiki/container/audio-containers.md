# Audio-only containers

Freestanding crates for containers with no video/track-graph concerns — each is a
small, self-contained mux + demux, no Mediaway dependency, added to round out
"commonly used" container coverage alongside MP4/WebM.

Each crate below also has a real FATE `fate_manifest.txt`/`demux_exceptions.rs`
(2026-07-29) — see [testing.md § FATE corpus](../../../conventions/testing.md).
Not all rows are `oracle_compare`: `riff-wave` compares `channels`/`sample_rate`
(not packetized), and `ogg`'s raw packet count includes Vorbis/Opus header
packets ffprobe's frame count excludes, so most `ogg` samples stay
`must_not_panic` rather than a forced/fragile offset match.

## `riff-wave` (WAV/PCM) — added 2026-07-29

Crate-local [ADR-0001](../../../crates/riff-wave/adr/0001-riff-wave-freestanding-core.md).

- Scope: PCM integer + IEEE float `fmt ` only (`wFormatTag` 1 / 3). No
  `WAVE_FORMAT_EXTENSIBLE`, no compressed WAV payloads (ADPCM, µ-law/A-law,
  MP3-in-WAV) — rejected via `Error::UnsupportedFormatTag`, never silently
  misread as PCM.
- `Muxer` buffers pushed samples; `finish()` writes the complete file — RIFF's
  `data` chunk size must be known before the header is written, so there is
  **no incremental flush** (a real format constraint, not a corner cut).
- `parse()` is a single-shot function over a complete buffer, symmetric with
  `Muxer::finish()` — no incremental `push_bytes`/`poll` demuxer, since RIFF has
  no fragmentation concept to stream against.
- Facade: `mediaway-container::wav` (2026-07-29) — mirrors this whole-buffer
  shape (`push_packet`/`finish`, `parse`) rather than the incremental
  `Mux`/`Demux` traits, which the format genuinely can't satisfy.

## `adts` (raw AAC elementary stream) — added 2026-07-29

Crate-local [ADR-0001](../../../crates/adts/adr/0001-adts-freestanding-core.md).

- No Mediaway or `iso-bmff` dependency — cross-checked field-for-field against
  `iso-bmff`'s pre-existing single-frame `strip_adts` helper
  (`bitstream/aac.rs`) so both agree on the wire format, but no shared code.
- `Muxer::write_frame` appends one self-contained frame per call (no container
  header, no `finish()` — unlike `riff-wave`, ADTS frames are genuinely
  independently streamable).
- `Demuxer` is a true incremental `push_bytes`/`poll_frame` reader, matching
  `iso-bmff`'s demux shape.
- Scope: no-CRC header only on the mux side (demux reads both no-CRC and
  CRC-protected headers); single raw-data-block per frame (the common AAC-LC case).
- Facade: `mediaway-container::adts` (2026-07-29) — real `Mux`/`Demux` impl;
  `pts`/`duration` synthesized from a running 1024-samples/frame count (ADTS
  carries no timing metadata itself).

## `mpeg-audio` (raw MP3 / Layer III elementary stream) — added 2026-07-29

Crate-local [ADR-0001](../../../crates/mpeg-audio/adr/0001-mpeg-audio-freestanding-core.md).

- Layer III only (Layer I/II rejected via `Error::UnsupportedLayer`, not
  misparsed) — "MP3" in the commonly-used sense. All three MPEG versions
  (1/2/2.5) supported, since low-bitrate MP3 commonly uses MPEG-2/2.5's
  half/quarter sample rates.
- Same framing-only philosophy as `adts`: `Muxer::write_frame` validates the
  already-encoded body length against the header's bitrate/sample-rate/padding,
  never fabricates or decodes audio data.
- Padding is a per-call parameter, not baked into the header config, since real
  encoders flip it per frame for bit-reservoir accounting.
- `Demuxer` is incremental (`push_bytes`/`poll_frame`), assumes frame-aligned
  input like `adts` (no ID3-tag/leading-garbage resync scan).
- Facade: `mediaway-container::mp3` (2026-07-29) — `Demux` only, no `Mux`
  trait impl: `write_frame`'s explicit `padding` argument has no slot in the
  generic `Packet`, and silently defaulting it would write wrong-length
  frames for real bit-reservoir-using streams. `CodecKind::Mp3` added.

## `ogg` (page/packet transport for Opus/Vorbis/FLAC) — added 2026-07-29

Crate-local [ADR-0001](../../../crates/ogg/adr/0001-ogg-freestanding-core.md).

- `Codec::Opus` already exists in `iso-bmff`'s `Codec` enum (for ISOBMFF muxing);
  this crate is the separate native Ogg transport, no shared code.
- Mux is intentionally simple: one packet per page, always spec-valid, no
  multi-packet batching or continuation-page splitting (packets over 65024 bytes
  are rejected, not split).
- Demux is fully general (must interoperate with real encoders' output
  regardless of what this crate's own mux produces): handles multiple packets
  per page and packets spanning continuation pages, verified against hand-built
  pages the mux itself never emits.
- Ogg's CRC-32 variant (non-reflected, poly `0x04C11DB7`) is implemented from
  scratch (~10 lines) rather than pulling in a `crc` crate dependency.
- Facade: `mediaway-container::ogg` (2026-07-29) — real `Mux`/`Demux` impl.
  Codec identified by reading the first packet's declared identification
  header (`OpusHead` magic, RFC 7845 §5.1; Vorbis ID header, Vorbis I §4.2.2)
  — the same "read a codec's own declared config bytes" boundary `iso-bmff`
  crosses for AAC's `esds`, not audio decoding. `granule_position` → `pts`
  directly (Opus: fixed 48 kHz per RFC 7845 §4; Vorbis: the stream's own
  rate). `CodecKind::Vorbis` added (also closed half of WebM's VP8/Vorbis
  gap — see [webm.md](webm.md)).
