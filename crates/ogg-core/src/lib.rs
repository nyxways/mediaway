//! Sans-IO Ogg page/packet mux and demux (RFC 3533) — no OS I/O, no Mediaway
//! types.
//!
//! - [`Muxer`] writes one page per packet (simple, always spec-valid — see
//!   crate-local ADR-0001 for what real-encoder features are deferred).
//! - [`Demuxer`] is a true incremental `push_bytes`/`poll_packet` reader that
//!   handles the general case: multiple packets per page, packets spanning
//!   continuation pages, and CRC verification.
//!
//! `Codec::Opus` already exists in `iso-bmff`'s `Codec` enum (for ISOBMFF
//! muxing) — this crate is the separate native Ogg transport (carries
//! Opus/Vorbis/FLAC logical bitstreams), independent of ISOBMFF.

#![forbid(unsafe_code)]

mod crc;
mod demux;
mod error;
mod mux;
mod types;

pub use crc::crc32_ogg;
pub use demux::Demuxer;
pub use error::Error;
pub use mux::{MAX_SINGLE_PAGE_PAYLOAD, Muxer};
pub use types::Packet;
