# ogg

Sans-IO Ogg page/packet mux + demux (RFC 3533). Freestanding — no Mediaway types.

Unprefixed reusable core — naming [ADR-0012](../../docs/adr/0012-unprefixed-reusable-cores.md).

v1 scope: mux is one-packet-per-page (simple, always valid); demux is fully
general (multi-packet pages, cross-page continuation). See
[`docs/roadmap.md`](docs/roadmap.md) and [`adr/0001`](adr/0001-ogg-freestanding-core.md).
