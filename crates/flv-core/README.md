# flv

Sans-IO FLV (Flash Video) tag mux + demux. Freestanding — no Mediaway types.

Unprefixed reusable core — naming [ADR-0012](../../docs/adr/0012-unprefixed-reusable-cores.md).

v1 scope: container framing only (file header + tag header + `PreviousTagSize`);
tag payload's codec-specific sub-framing is opaque. See
[`docs/roadmap.md`](docs/roadmap.md) and [`adr/0001`](adr/0001-flv-freestanding-core.md).
