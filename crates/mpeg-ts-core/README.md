# mpeg-ts

Sans-IO MPEG-2 Transport Stream mux + demux (ISO/IEC 13818-1). Freestanding —
no Mediaway types.

Unprefixed reusable core — naming [ADR-0012](../../docs/adr/0012-unprefixed-reusable-cores.md).

v1 scope: single program (one PAT entry, one PMT), `H264`/`Hevc`/`Aac`/`Mp3`
elementary streams, PTS/(optional DTS), no PCR. See
[`docs/roadmap.md`](docs/roadmap.md) and [`adr/0001`](adr/0001-mpeg-ts-freestanding-core.md).
