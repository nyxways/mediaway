//! Sans-IO MPEG-2 Transport Stream mux and demux (ISO/IEC 13818-1) — no OS
//! I/O, no Mediaway types.
//!
//! Single-program v1 scope (crate-local ADR-0001): one PAT entry, one PMT, a
//! handful of elementary streams. [`Muxer`] writes PAT/PMT + per-PID PES
//! packetization over 188-byte TS packets; [`Demuxer`] is a true incremental
//! `push_bytes`/`poll_access_unit` reader that tracks PAT/PMT and reassembles
//! PES packets per PID.
//!
//! Like `adts`/`mpeg-audio`/`ogg`/`flv`, this crate frames already-encoded
//! elementary-stream access units — it does not encode/decode H.264, HEVC, AAC,
//! or MP3 payloads.

#![forbid(unsafe_code)]

mod crc;
mod demux;
mod error;
mod mux;
mod packet;
mod pes;
mod psi;
mod types;

pub use demux::Demuxer;
pub use error::Error;
pub use mux::Muxer;
pub use types::{AccessUnit, ElementaryStream, StreamType};
