# rtmp

Sans-IO RTMP (Real-Time Messaging Protocol) publish-client handshake, chunk stream, and
AMF0 command mux. Freestanding — no Mediaway types.

Unprefixed reusable core — naming [ADR-0012](../../docs/adr/0012-unprefixed-reusable-cores.md).

**Status: implemented, v1 scope** — publish-client only: HMAC-SHA256 digest handshake, chunk
stream encode/decode, and a narrow AMF0 encode subset
(`connect`/`createStream`/`publish`/`onMetaData`). No AMF0 decode, no RTMPS/TLS, no server or
play (subscribe) role, no Enhanced RTMP v2 signaling yet. The handshake's digest-offset
formula is cross-checked against 3 independent implementations but **not yet exercised
against a real RTMP server** — see [`src/handshake.rs`](src/handshake.rs)'s module docs. See
[`docs/roadmap.md`](docs/roadmap.md) and [`adr/0001`](adr/0001-rtmp-freestanding-core.md).
