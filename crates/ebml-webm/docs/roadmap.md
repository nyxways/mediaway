# ebml-webm — roadmap

Sans-IO EBML/WebM demux (unprefixed). Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — EBML VINT + WebM demux subset (this session)

- [x] Crate + naming (ADR-0012) + [`adr/0001`](../adr/0001-ebml-vint-webm-schema-v1.md)
- [x] EBML VINT decode (element ID + element size, incl. unknown-size marker)
- [x] Element tree walk: `EBML` header (skipped), `Segment`, `Segment\Info`
      (`TimecodeScale`), `Tracks`/`TrackEntry` (`TrackNumber`, `TrackType`,
      `CodecID`, `Video\PixelWidth`/`PixelHeight`), `Cluster` (`Timecode`)
- [x] `SimpleBlock` → frame (track number, absolute timecode, keyframe flag)
- [x] Used by `mediaway-container::webm` (`Demux` only; codecs without a
      `CodecKind` mapping — VP8, Vorbis — are recognized structurally but
      omitted from the Mediaway-typed surface)

### 2 — Full Matroska profile elements (this session)

- [x] Crate-local [`adr/0002`](../adr/0002-full-matroska-profile.md)
- [x] Lacing: Xiph, fixed-size, and EBML (all three, not just one)
- [x] `BlockGroup` / `Block` / `BlockDuration` / `ReferenceBlock` (keyframe =
      absence of `ReferenceBlock`; `Frame::duration_ticks`)
- [x] `Audio\SamplingFrequency` (EBML Float) / `Channels` → `TrackInfo`
- [x] `Cues` / `SeekHead` — parsed and exposed informationally
      (`Demuxer::cues()`/`seek_head()`); this crate does no seeking itself

### 3 — Mux (2026-07-29)

- [x] Crate-local [`adr/0003`](../adr/0003-webm-mux.md)
- [x] `mux::Muxer<Open | Live>` — `EBML` header, `Segment` (always
      unknown-size), `Segment\Info\TimecodeScale`, `Tracks\TrackEntry`
      (`Video`/`Audio` sub-fields), `Cluster\Timecode` + `SimpleBlock`
      (known-size `Cluster` batches, no lacing/`BlockGroup` on write)
- [x] `vint::encode_id`/`encode_size`/`encode_unknown_size` — new public,
      low-level, total (never panic) encode counterparts to `decode_id`/`decode_size`
- [x] Verified by round-tripping through this crate's own `Demuxer`
      (`mux_tests.rs`) — no external WebM mux oracle available this session
- [x] Used by `mediaway-container::webm` (`Mux` impl, see that crate's roadmap)

### 4 — Close deferred gaps + VP8 (2026-08-05)

- [x] Crate-local [`adr/0004`](../adr/0004-cluster-lookahead-and-mux-lacing.md)
- [x] Sibling-ID lookahead closes an indefinite-size `Cluster` when a
      `Segment`-level sibling ID appears (`ids::is_segment_level_child`),
      instead of nesting it and growing the open-element stack once per
      `Cluster` — bounds the stack to `Segment` + one open `Cluster`
- [x] `Muxer::push_laced_frames` — EBML lacing writer (`vint::encode_signed_delta`,
      `vint::encode_size_fixed_len`, `lacing::encode_ebml_lace_sizes`)
- [x] `tests/mux_oracle.rs` — external ffprobe oracle on mux output (Tier 7),
      replacing "round-trip through own `Demuxer` only" as the sole check
- [x] `mediaway-common::CodecKind::Vp8` added and wired into
      `mediaway-container::webm::{codec_kind, webm_codec_id}` — closes the
      WebM VP8 gap (mux + demux); see `mediaway-container/docs/roadmap.md`
