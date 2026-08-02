# iso-bmff — roadmap

Freestanding ISOBMFF/MP4. Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — fMP4 + stbl demux

- [x] Typestate mux (`Open` → `Live`), ftyp/moov/moof/mdat
- [x] Demux: fMP4 + unfragmented `stbl` (mdat-before-moov)
- [x] ClearKey via `iso-cenc` (`tenc`/`senc`)
- [x] Conformance + FATE `oracle_compare`

### 2 — Hardening

- [x] Multi-elst packet expansion (`edts`/`elst`, FATE `mov-3elist` → 74 packets)
- [x] Discard / negative first-PTS (`mov_neg_first_pts_discard`: signed PTS + `is_discard`)
- [x] VP9 (`vp09`/`vpcC`) sample entry — [`adr/0002`](../adr/0002-vp9-sample-entry.md)
- [x] HEVC (`hvc1`/`hvcC`) + AV1 (`av01`/`av1C`) sample entries, honest `ftyp`
      compatible brands — [`adr/0003`](../adr/0003-hevc-av1-sample-entry.md)
- [ ] More codecs / sample entries as needed (`hev1` variant, `dvhe`/Dolby Vision, …)
