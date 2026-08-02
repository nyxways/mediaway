//! EBML / `WebM` parse error (`thiserror`).

#![forbid(unsafe_code)]

use thiserror::Error;

/// Low-level EBML/`WebM` parse error.
///
/// Used by the public low-level parse functions in [`crate::vint`]. The
/// stateful [`crate::Demuxer`] has no `Result`-returning `push_bytes` /
/// `poll_frame` (mirrors `iso_bmff::Demuxer`) — it handles malformed or
/// unsupported bytes internally by dropping data cleanly, never panicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Error {
    /// Buffer does not yet contain a full VINT / element header — wait for
    /// more bytes (not a malformed-data error).
    #[error("truncated EBML VINT or element header (need more bytes)")]
    Incomplete,
    /// The VINT length marker was not found within 8 bytes (reserved /
    /// invalid encoding). Unrecoverable at this buffer position.
    #[error("EBML VINT length marker not found within 8 bytes (reserved encoding)")]
    ReservedVint,
    /// A recognized but unimplemented EBML/`WebM` feature (e.g. `SimpleBlock`
    /// lacing, an over-long element ID). Named per call site.
    #[error("EBML/WebM feature not supported by this demuxer: {0}")]
    Unsupported(&'static str),
}

/// [`crate::mux::Muxer`] error.
///
/// Kept separate from [`Error`] (the low-level *parse*-error type) because
/// the two error sets are disjoint: nothing in `mux` ever returns [`Error`],
/// and nothing in `vint`/`demux` ever returns this type. Merging them would
/// force every exhaustive `match` on [`Error`] in the demux path to account
/// for mux-only variants it can never produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum MuxError {
    /// [`crate::mux::Muxer::add_track`]: `TrackNumber` `0` is reserved
    /// (Matroska/`WebM` forbid it as a real track number).
    #[error("WebM TrackNumber must be non-zero")]
    InvalidTrackNumber,
    /// [`crate::mux::Muxer::add_track`]: a track with this `TrackNumber` was
    /// already registered.
    #[error("duplicate WebM TrackNumber {0}")]
    DuplicateTrack(u64),
    /// [`crate::mux::Muxer::push_frame`]: `track_number` was never registered
    /// via `add_track`.
    #[error("push_frame references unregistered TrackNumber {0}")]
    UnknownTrack(u64),
}
