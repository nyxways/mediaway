//! Public error type.

#![forbid(unsafe_code)]

/// Errors from MPEG audio (Layer III) mux/demux.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// `bitrate_kbps` is not one of the 14 standard Layer III rates for this `MpegVersion`.
    #[error("bitrate {0} kbps has no Layer III bitrate index for this MPEG version")]
    UnsupportedBitrate(u16),
    /// `sample_rate` is not one of the 3 standard rates for this `MpegVersion`.
    #[error("sample rate {0} has no sampling_rate_index for this MPEG version")]
    UnsupportedSampleRate(u32),
    /// `frame_body.len()` doesn't match `FrameHeader::frame_len() - 4` (header size).
    #[error(
        "frame body length {actual} does not match the expected {expected} bytes for this header"
    )]
    FrameBodyLengthMismatch {
        /// Expected body length (`frame_len() - 4`).
        expected: usize,
        /// Actual `frame_body.len()` passed in.
        actual: usize,
    },
    /// The next 3 bytes are not a valid frame sync + version/layer/bitrate/samplerate combination.
    #[error("bad MPEG audio frame sync or reserved header field")]
    BadSyncOrReservedField,
    /// Layer is not Layer III (`01`) — only Layer III is supported by this crate.
    #[error("unsupported layer (only Layer III is implemented)")]
    UnsupportedLayer,
}
