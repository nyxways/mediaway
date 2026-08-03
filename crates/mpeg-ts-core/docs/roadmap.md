# mpeg-ts — roadmap

Sans-IO MPEG-2 Transport Stream mux + demux (unprefixed). Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — Single-program mux + demux (this session)

- [x] Crate + naming (ADR-0012) + [`adr/0001`](../adr/0001-mpeg-ts-freestanding-core.md)
- [x] 188-byte TS packet write/parse with correct adaptation-field stuffing
      (a real bug — missing mandatory flags byte on pure-padding fields — was
      found and fixed via this crate's own round-trip tests)
- [x] PAT/PMT build + parse (MPEG-2 PSI CRC-32 variant, from scratch)
- [x] PES packetization with PTS-only or PTS+DTS (33-bit bit-packed timestamps)
- [x] `Muxer`: `write_pat_pmt` + `write_access_unit` per PID
- [x] `Demuxer`: incremental `push_bytes`/`poll_access_unit`, tracks PAT/PMT,
      reassembles PES per PID, `finish()` flushes the last access unit per PID

### Deferred (tracked, not silently dropped)

- [ ] Multi-program transport streams (one PAT entry only)
- [ ] PCR insertion/extraction (`PCR_PID` always written as `0x1FFF`, unassigned)
- [ ] Multi-packet PSI section reassembly (PAT/PMT spanning >1 TS packet)
- [ ] DTS-only access units (every access unit needs at least a PTS)
- [x] `mediaway-container` facade wiring — `mediaway-container::ts` (`Demux`
      only; mux exposes `write_access_unit(pid, data, pts_90k, dts_90k, ...)`
      directly rather than a `Mux` trait impl — MPEG-TS's fixed 90 kHz clock
      isn't a per-track `Rational`, so silently reinterpreting `Packet::pts`
      would risk desync) — 2026-07-29
