# mediaway-container — roadmap

Facade for container formats. Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — Traits + MP4 surface

- [x] `ContainerFormat` / `Mux` / `Demux` / `DemuxDecrypt`
- [x] Mediaway-typed `mp4` over `iso-bmff`

### 2 — More formats

- [x] WebM demux behind `Demux` — `ebml-webm` core + `mediaway-container::webm`
      (ADR: `adr/0001-webm-ebml-demux.md`). Demux only; VP8/Vorbis tracks are
      recognized structurally but not yet representable via `CodecKind`
      (see `ebml-webm/adr/0001` and `ebml-webm/docs/roadmap.md` for the exact
      subset and deferred items: lacing, `BlockGroup`/duration, indefinite
      `Cluster` lookahead).
- [x] `BlockGroup`'s `BlockDuration` threaded into `Packet::duration`
      (`ebml_webm::Frame::duration_ticks` was already exposed by the core;
      only the facade wiring was missing) — 2026-07-29.
- [x] `sample_rate`/`channels` facade surface — `StreamInfo::Audio`
      (`mediaway-common`) gained real `sample_rate`/`channels` fields (shared
      with `mp4.rs`, `mediaway-device-windows` WASAPI, `mediaway-encoder-windows`
      AAC); `webm.rs::to_stream_info` now threads real values through instead
      of dropping them — 2026-07-29.
- [x] `cues`/`seek_head` facade surface — `Demuxer::cues()`/`seek_head()`
      re-export `ebml_webm::{CuePoint, SeekEntry}` directly (no conversion
      needed, plain seek-index offsets) — 2026-07-29.
- [x] `CodecKind::Vorbis` added (for the Ogg facade, see Stage 3) and wired
      into `webm.rs::codec_kind` — closes half of the VP8/Vorbis gap.
- [x] WebM mux — `ebml-webm` gained a real `mux::Muxer` (`ebml-webm/adr/0003`)
      and `webm::Muxer<Open | Live>` wraps it as a full `crate::Mux` impl
      (`adr/0003-webm-mux-facade.md`), symmetric with `mp4::Muxer`'s typestate
      shape. Same codec set as demux (`Vp9`/`Av1`/`Opus`/`Vorbis`/`Aac`);
      anything else rejected at `add_track` — 2026-07-29.
- [x] `CodecKind::Vp8` added — closes the other half of the VP8/Vorbis gap.
      Wired into `webm.rs::codec_kind`/`webm_codec_id` (demux and mux both);
      every other exhaustive match on `CodecKind` across the workspace
      updated to a correct arm (mostly "unsupported codec" — no backend
      implements VP8 encode/decode) — see `mediaway-common`'s and
      `mediaway-ffi`'s roadmaps for the FFI mirror-enum discriminant.
- [ ] Optional facade features to re-export a default format

### 3 — `riff-wave`/`adts`/`mpeg-audio`/`ogg`/`flv`/`mpeg-ts` facades

- [x] All 6 freestanding cores wired: `adts.rs`, `wav.rs`, `mp3.rs`, `ogg.rs`,
      `flv.rs`, `ts.rs` — ADR: `adr/0002-audio-and-general-container-facades.md`.
      `Demux` implemented for all 6; `Mux` implemented for `adts`/`ogg` only
      (see ADR for why `mpeg_audio`/`flv`/`mpeg_ts`/`riff_wave` expose their
      own method shapes instead — 2026-07-29.
- [x] New `CodecKind` variants: `Mp3`, `Vorbis` (`RawAudio`, added earlier
      this session for WASAPI capture, reused for `riff_wave` PCM).
- [x] `flv` mux gained a codec-aware convenience layer symmetric with demux —
      `Muxer::add_track` + `Muxer::push_packet` build the
      `AudioTagHeader`/`VideoTagHeader` sub-framing (AVC video, AAC/MP3
      audio) and write the sequence-header tag once before the first data
      tag per track; still its own method shape, not `crate::Mux` (see
      `adr/0002`) — 2026-07-29.
- [ ] `ogg` `Vorbis`/`Opus` support is identification-header-only — no
      page-batching (multiple packets per page) on the mux side (matches
      `ogg` crate's own v1 scope, ADR `ogg/adr/0001`).
