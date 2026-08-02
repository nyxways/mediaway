# flv — roadmap

Sans-IO FLV tag mux + demux (unprefixed). Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — Container framing (this session)

- [x] Crate + naming (ADR-0012) + [`adr/0001`](../adr/0001-flv-freestanding-core.md)
- [x] `Muxer::write_header` + `write_tag` (no `finish()` — tags self-trail their own `PreviousTagSize`)
- [x] `Demuxer::poll_tag`: incremental, reads `DataOffset` instead of hardcoding
      9 bytes, tested byte-by-byte across header/tag boundaries

### Deferred (tracked, not silently dropped)

- [ ] AudioTagHeader/VideoTagHeader sub-framing (`AACPacketType`,
      `AVCPacketType`, composition time) — `Tag::data` stays opaque in this
      core crate by design (see module docs); `mediaway-container::flv`
      reads/builds these bytes on both the demux and mux sides.
- [ ] Script-data (AMF0/AMF3) parsing — framed but not decoded
- [x] `mediaway-container` facade wiring — `mediaway-container::flv`
      (`Demux`: AVC video + AAC/MP3 audio recognized; `Mux`: codec-aware
      `add_track`/`push_packet` build the matching sequence-header/data tags
      for the same codec set, own method shape per ADR-0002) — 2026-07-29
