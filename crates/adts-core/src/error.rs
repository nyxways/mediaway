//! Public error type.

#![forbid(unsafe_code)]

/// Errors from ADTS mux/demux.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// `sample_rate` is not one of the 13 standard ADTS rates.
    #[error("sample rate {0} has no ADTS sampling_frequency_index")]
    UnsupportedSampleRate(u32),
    /// Raw AAC payload is too large: `7 + payload.len()` must fit the 13-bit `aac_frame_length` field (max 8191).
    #[error(
        "frame too large for the 13-bit ADTS length field: header + payload = {0} bytes (max 8191)"
    )]
    FrameTooLarge(usize),
    /// The next 2 bytes are not `0xFF F*` (ADTS syncword + MPEG version/layer/protection bits).
    #[error("bad ADTS sync word")]
    BadSync,
    /// `sampling_frequency_index` in the header is reserved (13/14) or "explicit frequency" (15).
    #[error("reserved/unsupported sampling_frequency_index {0}")]
    UnsupportedSamplingFrequencyIndex(u8),
}
