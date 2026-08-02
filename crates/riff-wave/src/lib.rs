//! Sans-IO RIFF/WAVE (PCM) mux and demux — no OS I/O, no Mediaway types.
//!
//! - [`Muxer`] buffers pushed PCM samples and writes a complete RIFF/WAVE file on
//!   [`Muxer::finish`] — RIFF's `data` chunk size must be known before the header is
//!   written, so (unlike `iso-bmff`'s fragmented fMP4 output) there is no incremental
//!   flush here.
//! - [`parse`] reads a complete RIFF/WAVE buffer back into a [`WaveFormat`] and raw
//!   PCM [`bytes::Bytes`] payload.

#![forbid(unsafe_code)]

mod demux;
mod error;
mod mux;
mod types;

pub use demux::parse;
pub use error::Error;
pub use mux::Muxer;
pub use types::{SampleFormat, WaveFormat};
