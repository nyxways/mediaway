//! Sans-IO ISOBMFF / MP4 — freestanding mux and demux.
//!
//! Callers own all I/O. No Mediaway types. `ClearKey` sample crypto via [`iso_cenc`].

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

pub mod bitstream;
mod codec_features;
pub mod error;
pub mod isobmff;
pub mod types;

#[cfg(all(not(feature = "mux"), not(feature = "demux")))]
compile_error!("enable at least one of `mux` or `demux` features on iso-bmff");

#[cfg(feature = "demux")]
pub mod demux;
#[cfg(feature = "mux")]
pub mod mux;

/// Typical A/V (+ a couple extras) — stack capacity for track tables.
pub const INLINE_TRACKS: usize = 4;
/// Per-fragment sample rows on the stack (covers default fragment batch of 30).
pub const INLINE_SAMPLES: usize = 32;

/// Mux/demux error.
pub use error::Error;

#[cfg(feature = "demux")]
pub use demux::Demuxer;
#[cfg(feature = "mux")]
pub use mux::{DEFAULT_FRAGMENT_BATCH, Live, Muxer, Open};
pub use types::{Bytes, Codec, Rational, Sample, Track};
