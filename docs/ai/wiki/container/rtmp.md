# `rtmp` — RTMP publish-client protocol core (implemented)

Crate-local [ADR-0001](../../../../crates/rtmp/adr/0001-rtmp-freestanding-core.md).
Unprefixed freestanding core (ADR-0012): HMAC-SHA256 digest handshake (C0/C1/C2), chunk-stream
encode/decode, and a narrow AMF0 **encode** subset
(`connect`/`createStream`/`publish`/`onMetaData`). Sans-io — byte slices in/out, no
`std::net`, no socket anywhere in the crate. `#![forbid(unsafe_code)]` (zero `unsafe`; `hmac`/
`sha2` are the only new deps).

## Public shape

```text
Handshake::new() -> Self                          // queues C0+C1 immediately
Handshake::pending_send(&self) -> &[u8]
Handshake::advance_send(&mut self, n: usize)
Handshake::feed_recv_bytes(&mut self, &[u8]) -> Result<(), Error>   // diverges from ADR: fallible
Handshake::is_complete(&self) -> bool

Muxer::new(chunk_size: u32) -> Self
Muxer::write_connect/write_create_stream/write_publish/write_metadata(..., &mut Vec<u8>)
Muxer::push_video_data/push_audio_data(&[u8], timestamp_ms, &mut Vec<u8>)

Demuxer::push_bytes(&mut self, &[u8])
Demuxer::poll_message(&mut self) -> Result<Option<(u8, u32, Vec<u8>)>, Error>  // diverges: fallible

ChunkEncoder, ChunkDecoder, amf0::{write_number, write_boolean, write_string, write_null,
  write_object_start, write_property_name, write_object_end, write_ecma_array_start}
```

`ChunkEncoder`/`ChunkDecoder` and `amf0` are public (not just `Muxer`/`Demuxer` internals) —
low-level APIs stay usable per `docs/spec/api-layers.md`.

## Two deliberate deviations from ADR-0001 § 4's literal signatures

`Handshake::feed_recv_bytes` and `Demuxer::poll_message` return `Result<_, Error>` rather
than the ADR's bare `()`/`Option<_>` — both can genuinely fail on malformed/adversarial input
(bad `S0` version, undecodable `S1` digest, a type-3 chunk with no cached header, an invalid
`Set Chunk Size`), and this workspace's own `flv::Demuxer::poll_tag` already uses the same
`Result<Option<T>, Error>` idiom rather than silently swallowing a real parse error. Documented
in each module's rustdoc.

## Handshake — digest-offset formula, sources and confidence

The complex handshake places its 32-byte digest inside the 1536-byte `C1`/`S1` block at a
position computed from a **layout**-dependent formula (community-numbered "scheme 0/1"
inconsistently across sources — `src/handshake.rs` names them by structure instead:
**digest-first** vs **key-first**). Neither formula is in a redistributable Adobe spec; this
implementation cross-checked **3 independently authored sources** before writing the module:

1. FFmpeg `rtmpproto.c` (via an annotated community gist)
2. `librtmp`/`rtmpdump`'s `handshake.h` (`GetDigestOffset1`/`GetDigestOffset2`)
3. SRS (`ossrs/srs`) — an independent from-scratch C++ RTMP server

All three agree **exactly**: digest-first → `12 + (sum of bytes[8..12] mod 728)`; key-first →
`776 + (sum of bytes[772..776] mod 728)`. Same 3 sources also confirmed the HMAC key
constants (`GenuineFPKey`/`GenuineFMSKey`, byte-identical across `librtmp` and SRS) and the
`C2`/`S2` digest derivation (fixed-position, no offset ambiguity).

**Confidence: high** on the byte-level formula (3-source agreement). **Not verified**: real
RTMP server interop (YouTube/Twitch/nginx-rtmp/SRS live instance) — only cross-checked
against reference source and this crate's own self-consistency tests (own C1 math used to
build a synthetic-but-compliant S1/S2, fed back through `Handshake` and accepted). This is
the same named risk in `adr/0001` § Consequences, not resolved by this implementation.

## Payload boundary

`push_video_data`/`push_audio_data` take already-built FLV-tag-body bytes
(`VideoTagHeader`+NALU, `AudioTagHeader`+AAC/MP3) — the same shapes
`mediaway-container::flv`'s private builder functions produce, but this crate does not depend
on `flv` or any `mediaway-*` type (freestanding core boundary). A future Mediaway-typed
adapter (`Packet` → these bytes) is out of scope here — see `adr/0001` § 5.

## Chunk stream assumptions worth knowing

- `Muxer` assumes the server assigns stream ID `1` to the first `createStream` (no AMF0
  decode means it can't read the real `_result` payload) — documented, not guaranteed.
- `Muxer` emits `Set Chunk Size` automatically before its first other message; `ChunkDecoder`
  recognizes and applies it internally while still surfacing it via `poll_message` like any
  other message.
- Direct `ChunkEncoder`/`ChunkDecoder` composition (bypassing `Muxer`/`Demuxer`) must keep
  `chunk_size` in sync manually (`ChunkDecoder::set_chunk_size`) if not exchanging the control
  message — a real protocol stream always negotiates this via the wire message instead.
