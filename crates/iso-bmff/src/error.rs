//! Container error type (`thiserror`).

#![forbid(unsafe_code)]

use thiserror::Error;

/// MP4 mux/demux error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Error {
    /// Track id missing or duplicate.
    #[error("invalid track id or duplicate registration")]
    InvalidTrack,
    /// Sample does not match a registered track / bad framing.
    #[error("invalid Sample for registered tracks")]
    InvalidPacket,
    /// Truncated or malformed ISOBMFF.
    #[error("invalid or truncated ISOBMFF data")]
    InvalidData,
}
