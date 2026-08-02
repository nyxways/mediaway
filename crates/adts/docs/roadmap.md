# adts — roadmap

Sans-IO ADTS (raw AAC) mux + demux (unprefixed). Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — ADTS mux + demux (this session)

- [x] Crate + naming (ADR-0012) + [`adr/0001`](../adr/0001-adts-freestanding-core.md)
- [x] `Muxer::write_frame`: one no-CRC (7-byte header) ADTS frame per call
- [x] `Demuxer`: incremental `push_bytes`/`poll_frame`, waits on partial frames,
      reads both no-CRC (7-byte) and CRC (9-byte) headers
- [x] Bit layout cross-checked against `iso-bmff`'s existing `strip_adts`

### Deferred (tracked, not silently dropped)

- [ ] Muxer-side CRC (9-byte header) support — demux already reads it, mux only
      ever writes no-CRC
- [ ] Multi-raw-data-block-per-frame exposure (payload bytes survive; the crate
      doesn't expose the internal AAC-frame boundary within one ADTS frame)
- [x] `mediaway-container` facade wiring — `mediaway-container::adts`
      (`Mux`/`Demux`, `pts`/`duration` synthesized from a 1024-samples/frame
      count) — 2026-07-29
