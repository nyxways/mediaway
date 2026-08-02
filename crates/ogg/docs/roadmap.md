# ogg — roadmap

Sans-IO Ogg page/packet mux + demux (unprefixed). Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — Page mux + general demux (this session)

- [x] Crate + naming (ADR-0012) + [`adr/0001`](../adr/0001-ogg-freestanding-core.md)
- [x] From-scratch Ogg CRC-32 variant (poly `0x04C11DB7`, non-reflected)
- [x] `Muxer::push_packet`: one packet per page, correct lacing + CRC, bos/eos flags
- [x] `Demuxer::poll_packet`: incremental, handles multi-packet pages and
      cross-page packet continuation (the general case, tested against
      hand-built pages even though this crate's own mux never produces them)

### Deferred (tracked, not silently dropped)

- [ ] Mux: multi-packet-per-page batching (real encoders pack small packets
      together for efficiency; this crate always emits one packet = one page)
- [ ] Mux: continuation-page splitting for packets over 65024 bytes
      (`Error::PacketTooLargeForSinglePage` today)
- [ ] Multi-logical-stream interleaving/chaining (one `Muxer` = one `serial`)
- [x] `mediaway-container` facade wiring — `mediaway-container::ogg`
      (`Mux`/`Demux`; codec identified from the first packet's `OpusHead`/
      Vorbis identification header) — 2026-07-29
