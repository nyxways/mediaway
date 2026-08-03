# riff-wave — roadmap

Sans-IO RIFF/WAVE PCM mux + demux (unprefixed). Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — PCM mux + demux (this session)

- [x] Crate + naming (ADR-0012) + [`adr/0001`](../adr/0001-riff-wave-freestanding-core.md)
- [x] `Muxer`: buffered PCM samples → complete `RIFF`/`WAVE`/`fmt `/`data` file on `finish()`
- [x] `parse()`: complete-buffer demux, skips unknown chunks (`LIST`, `fact`, …)
- [x] `wFormatTag` 1 (PCM) and 3 (IEEE float)

### Deferred (tracked, not silently dropped)

- [ ] `WAVE_FORMAT_EXTENSIBLE` (`wFormatTag` 0xFFFE) — multichannel channel masks
- [ ] Compressed WAV payloads (ADPCM, µ-law/A-law, MP3-in-WAV) — would need a
      per-format decode step outside a container crate's scope
- [x] `mediaway-container` facade wiring — `mediaway-container::wav`
      (`push_packet`/`finish`, `parse`; no `Mux`/`Demux` trait fit, RIFF's
      chunk sizes need the whole buffer up front) — 2026-07-29
- [ ] RF64/W64 (>4 GiB WAV) — out of scope; classic 32-bit RIFF size field only
