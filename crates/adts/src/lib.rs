//! Sans-IO ADTS (raw AAC elementary stream) mux and demux — no OS I/O, no
//! Mediaway types.
//!
//! ADTS has no container-level header — it is just a concatenation of
//! self-describing frames (7-byte header, no CRC, single raw-data-block-per-frame —
//! the common case for AAC-LC streams). [`Muxer::write_frame`] appends one frame
//! per call (no `finish()`); [`Demuxer`] is a true incremental `push_bytes`/`poll`
//! reader, matching `iso-bmff`'s demux shape.
//!
//! `iso-bmff` already has a one-off ADTS-header-strip helper
//! (`crates/iso-bmff/src/bitstream/aac.rs`) for muxing a single AAC frame into MP4;
//! this crate is the standalone ADTS *container* (mux + incremental demux over a
//! byte stream), independent of ISOBMFF.

#![forbid(unsafe_code)]

mod demux;
mod error;
mod mux;
mod types;

pub use demux::Demuxer;
pub use error::Error;
pub use mux::Muxer;
pub use types::{AacProfile, AdtsConfig};
