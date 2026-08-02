# General-purpose containers beyond MP4/WebM

Freestanding crates for widely-used general (audio+video) containers, added to
round out "commonly used" container coverage alongside MP4/WebM and the
audio-only containers ([audio-containers](audio-containers.md)).

Both crates below also have a real FATE `fate_manifest.txt`/`demux_exceptions.rs`
(2026-07-29) — see [testing.md § FATE corpus](../../../conventions/testing.md).
`flv` counts only `Audio`/`Video` tags (excludes `ScriptData`) to match
ffprobe's semantics. `mpeg-ts` oracle rows need ffprobe's CSV de-duplicated —
`-count_packets` prints every MPEG-TS stream twice (program-grouped view +
flat view), confirmed via `-of json`; a real ffprobe quirk, not a bug here.

## `flv` (Flash Video) — added 2026-07-29

Crate-local [ADR-0001](../../../crates/flv/adr/0001-flv-freestanding-core.md).

- Scope: the FLV **container** structure only (file header, tag header,
  `PreviousTagSize` trailer) — `Tag::data` is opaque already-formatted payload;
  this crate does not interpret `AudioTagHeader`/`VideoTagHeader` sub-framing
  (`AACPacketType`, `AVCPacketType`, composition time) or AMF script-data.
- `Muxer` has no `finish()` — tags are independently appendable, each
  self-trailed with its own `PreviousTagSize`.
- `Demuxer` reads the header's `DataOffset` field rather than hardcoding the
  common 9-byte size, and is a true incremental `push_bytes`/`poll_tag` reader
  (tested byte-by-byte across header/tag boundaries).
- Facade: `mediaway-container::flv`. Demux reads the `AudioTagHeader`/
  `VideoTagHeader` sub-bytes this core crate leaves opaque (`SoundFormat`/
  `CodecID`, `AACPacketType`/`AVCPacketType`, composition time) to split
  sequence-header tags (→ `extra_data`) from data tags (→ `Packet`). Only AVC
  video and AAC/MP3 audio recognized — other `CodecID`/`SoundFormat` values
  (VP6, Sorenson H.263, Nellymoser, …) have no `CodecKind` mapping, same
  posture as WebM's VP8 gap. **Mux gained the symmetric codec-aware layer
  (2026-07-29)**: `Muxer::add_track`/`push_packet` write the sequence-header
  tag once per track before data tags; unsupported codecs rejected at
  `add_track`, not silently dropped. Still its own method shape
  (`push_packet(&Packet, &mut Vec<u8>)`), not the shared `Mux` trait.

## `mpeg-ts` (MPEG-2 Transport Stream) — added 2026-07-29

Crate-local [ADR-0001](../../../crates/mpeg-ts/adr/0001-mpeg-ts-freestanding-core.md).
Largest of the newly-added container crates.

- Single-program v1 scope: one PAT entry, one PMT, `H264`/`Hevc`/`Aac`/`Mp3`
  elementary streams.
- Module split mirrors the format's own layering: `packet.rs` (188-byte TS
  packets + adaptation-field stuffing), `psi.rs` (PAT/PMT), `pes.rs` (PES +
  PTS/DTS bit-packing), `crc.rs` (MPEG-2 PSI CRC-32 — same polynomial as `ogg`'s
  CRC but a **different init value**, not interchangeable).
- This crate's own mux↔demux round-trip tests caught one real bug before it
  shipped: the adaptation field's flags byte is mandatory whenever
  `adaptation_field_length > 0` (even pure-padding fields), not only when
  `random_access_indicator` needed setting — the first draft produced 189-byte
  "packets" until fixed.
- No PCR insertion (`PCR_PID` always `0x1FFF`, unassigned); no multi-program
  support; PSI sections spanning more than one TS packet aren't reassembled.
- Facade: `mediaway-container::ts` (2026-07-29) — `Demux` only; `StreamType`
  (from PMT) maps directly to `CodecKind`, `time_base` fixed at `1 / 90_000`
  (the real MPEG-TS system clock, not a per-track choice). Mux exposes
  `write_access_unit(pid, data, pts_90k, dts_90k, ...)` directly rather than
  the generic `Mux` trait — reinterpreting `Packet::pts` (arbitrary time
  base) as a 90 kHz value would risk silent desync.
