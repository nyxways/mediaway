//! Sans-IO RTMP (Real-Time Messaging Protocol) publish-client handshake, chunk stream, and
//! AMF0 command mux — no OS I/O, no Mediaway types.
//!
//! - [`Handshake`] — C0/C1/C2 ⇄ S0/S1/S2, HMAC-SHA256 digest variant only, byte-slice in/out
//!   (no socket). See its module docs for the digest-offset formula's sources and
//!   confidence: cross-checked against 3 independent implementations, but **not** yet
//!   exercised against a real RTMP server — see `handshake`'s rustdoc caveat.
//! - [`Muxer`] — chunk-stream encode + AMF0 command encode (`connect`/`createStream`/
//!   `publish`/`onMetaData`) + `push_video_data`/`push_audio_data` (raw, already
//!   FLV-tag-body-shaped payload bytes in, chunked RTMP bytes appended to a caller-owned
//!   `Vec<u8>` out — no `finish()`, same push-append shape as `flv::Muxer`).
//! - [`Demuxer`] — chunk-stream decode only (`push_bytes`/`poll_message`), producing raw
//!   `(message_type_id, timestamp_ms, payload)` tuples. **No AMF0 decode** — a
//!   publish-only client does not need to parse command *contents* to make progress
//!   (explicit scope cut, see `adr/0001` § 3).
//! - [`amf0`] and [`ChunkEncoder`]/[`ChunkDecoder`] are the lower-level primitives `Muxer`/
//!   `Demuxer` compose; they stay public per this workspace's "low-level APIs stay usable"
//!   rule ([`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md)).
//!
//! **Status: implemented**, per `adr/0001-rtmp-freestanding-core.md`. Handshake correctness
//! is cross-checked against reference source (3 independent implementations) and this
//! crate's own self-consistency tests — **not** against a real RTMP server. Treat the
//! handshake as unverified for production interop until that gate is run; see
//! [`Handshake`]'s module docs for the full sourcing/confidence writeup.
//!
//! Non-goals (v1): sockets/TCP transport, RTMPS/TLS, server or play (subscribe) role,
//! Enhanced RTMP v2 (FourCC/HEVC/AV1 signaling), reconnect/backoff policy, AMF0/AMF3
//! decode, and any dependency on `mediaway-*` crates/types (freestanding core, per
//! [ADR-0012](../../../docs/adr/0012-unprefixed-reusable-cores.md)).
#![forbid(unsafe_code)]

mod chunk_common;
mod chunk_decoder;
mod chunk_encoder;
mod demux;
mod error;
mod handshake;
mod mux;
mod types;

pub mod amf0;

pub use chunk_decoder::ChunkDecoder;
pub use chunk_encoder::ChunkEncoder;
pub use demux::Demuxer;
pub use error::Error;
pub use handshake::Handshake;
pub use mux::Muxer;
pub use types::OnMetaData;
