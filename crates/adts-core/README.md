# adts

Sans-IO ADTS (raw AAC elementary stream) mux + demux. Freestanding — no Mediaway
or `iso-bmff` types.

Unprefixed reusable core — naming [ADR-0012](../../docs/adr/0012-unprefixed-reusable-cores.md).

v1 scope: no-CRC header, single raw-data-block per frame (the common AAC-LC case).
See [`docs/roadmap.md`](docs/roadmap.md) and
[`adr/0001`](adr/0001-adts-freestanding-core.md).
