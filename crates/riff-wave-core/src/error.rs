//! Public error type.

#![forbid(unsafe_code)]

/// Errors from RIFF/WAVE demux.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The leading 12 bytes are not a `RIFF`/`WAVE` header.
    #[error("not a RIFF/WAVE container")]
    NotRiffWave,
    /// No `fmt ` chunk found before end of input.
    #[error("missing fmt chunk")]
    MissingFmtChunk,
    /// `fmt ` chunk is present but shorter than the 16-byte PCM `fmt ` body.
    #[error("truncated fmt chunk (need at least 16 bytes)")]
    TruncatedFmtChunk,
    /// `fmt ` chunk's `wFormatTag` is not PCM (1) or IEEE float (3).
    #[error("unsupported wFormatTag {0} (only PCM=1 / IEEE float=3 supported)")]
    UnsupportedFormatTag(u16),
}
