# rtmp — roadmap

Sans-IO RTMP publish-client handshake + chunk stream + AMF0 command mux (unprefixed).
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Crate + naming (ADR-0012) + [`adr/0001`](../adr/0001-rtmp-freestanding-core.md)
  (**Status: Accepted** — `hmac`/`sha2` added to `[workspace.dependencies]`, `cargo deny
  check` run clean)

### 1 — Handshake

- [x] Add `hmac` + `sha2` to `[workspace.dependencies]`; `cargo deny check` run clean
- [x] `Handshake`: C0/C1/C2 HMAC-SHA256 digest variant, sans-io (`feed_recv_bytes` /
  `pending_send` / `advance_send` / `is_complete`; `feed_recv_bytes` returns `Result<(),
  Error>`, a documented deviation from the ADR's literal signature — see `src/handshake.rs`
  module docs)
- [x] Digest-offset formula cross-checked against 3 independent implementations (FFmpeg,
  `librtmp`, SRS) — high confidence in the byte-level formula; **not yet exercised against a
  real RTMP server** (self-consistency tests only, see `src/handshake_tests.rs`). Real-server
  interop remains the outstanding correctness gate before production use.

### 2 — Chunk stream

- [x] `ChunkEncoder`: basic header, message header types 0-3, extended timestamp,
  `chunk_size`-bounded fragmentation
- [x] `ChunkDecoder`: incremental `push_bytes`/`poll_message` reassembly, tested byte-by-byte
  across chunk boundaries (mirrors `flv::Demuxer`'s own boundary tests)

### 3 — AMF0 command mux + connect/publish flow

- [x] AMF0 encode subset: Number/Boolean/String/Object/Null/ECMA Array (no decode — see
  `adr/0001` § 3)
- [x] `Muxer::write_connect` / `write_create_stream` / `write_publish` / `write_metadata`
- [x] `Muxer::push_video_data` / `push_audio_data` — raw already-FLV-tag-body-shaped bytes in,
  chunked RTMP message bytes out (see `adr/0001` § Payload boundary)
- [ ] Real handshake + connect + publish smoke test against a local RTMP-compatible server
  (e.g. an open-source media server on PATH, optional/skip-if-absent — same posture as the
  FFmpeg test oracle). **Not done this session** — the blocking correctness gate named in
  `adr/0001` § Consequences remains open.

### Deferred (tracked, not silently dropped)

- [ ] AMF0/AMF3 decode
- [ ] RTMPS/TLS transport
- [ ] Server role / play (subscribe) role
- [ ] Enhanced RTMP v2 (FourCC, HEVC/AV1 signaling) — phase-3 follow-up ADR
- [ ] Legacy "simple" (non-digest) handshake fallback
- [ ] `mediaway-container::flv` → `rtmp::Muxer` adapter (Packet → FLV-tag-body bytes) — a
  future Mediaway-typed crate/module, not this freestanding core (see `adr/0001` § 5)
