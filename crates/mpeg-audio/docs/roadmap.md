# mpeg-audio — roadmap

Sans-IO MPEG-1/2/2.5 Layer III mux + demux (unprefixed). Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — Layer III mux + demux (this session)

- [x] Crate + naming (ADR-0012) + [`adr/0001`](../adr/0001-mpeg-audio-freestanding-core.md)
- [x] `Muxer::write_frame`: 4-byte Layer III header + already-encoded body, per-call padding
- [x] `Demuxer`: incremental `push_bytes`/`poll_frame`, waits on partial frames,
      reads both no-CRC and CRC headers
- [x] MPEG-1, MPEG-2, MPEG-2.5 sample-rate/bitrate tables
- [x] Frame-length formula cross-checked against the standard reference value
      (128 kbps/44100 Hz/no-padding → 417 bytes)

### Deferred (tracked, not silently dropped)

- [ ] Layer I / Layer II — rejected via `Error::UnsupportedLayer`, not implemented
- [ ] Muxer-side CRC support — demux already reads it, mux only ever writes no-CRC
- [ ] ID3v1/ID3v2 tag skip on demux — assumes frame-aligned input, no resync scan
- [ ] Xing/VBR header recognition (first-frame side metadata, not a real audio frame)
- [x] `mediaway-container` facade wiring — `mediaway-container::mp3` (`Demux`
      only; no `Mux` trait fit — real streams need an explicit per-frame
      `padding` bit the generic `Packet` has no slot for) — 2026-07-29
