//! Sans-IO EBML / `WebM` — freestanding demux + mux (see `adr/0001`,
//! `adr/0003`).
//!
//! Callers own all I/O. No Mediaway types. Mediaway-typed wiring lives in
//! `mediaway_container::webm`.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

mod demux;
pub mod error;
pub mod ids;
mod lacing;
pub mod mux;
pub mod types;
pub mod vint;

/// Open-element stack depth on the stack (`EBML`/`Segment`/`Info` or
/// `Tracks`/`TrackEntry`/`Video` or `Cluster`) — typically ≤4.
pub const INLINE_STACK: usize = 6;
/// Typical track count table capacity (audio + video, a couple extras).
pub const INLINE_TRACKS: usize = 4;
/// Typical `Cues`/`SeekHead` entry-table capacity.
pub const INLINE_INDEX: usize = 8;

pub use demux::Demuxer;
/// Low-level parse error (`vint`/`demux`) — see [`error::Error`] docs.
pub use error::Error;
/// [`mux::Muxer`] error — see [`error::MuxError`] docs.
pub use error::MuxError;
pub use mux::Muxer;
pub use types::{Bytes, CuePoint, Frame, Rational, SeekEntry, TrackInfo};
