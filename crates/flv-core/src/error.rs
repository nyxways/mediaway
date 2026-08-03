//! Public error type.

#![forbid(unsafe_code)]

/// Errors from FLV mux/demux.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// First 3 bytes are not `FLV`.
    #[error("bad FLV signature (expected \"FLV\")")]
    BadSignature,
    /// `TagType` byte is not 8 (audio), 9 (video), or 18 (script data).
    #[error("unknown FLV tag type {0} (expected 8, 9, or 18)")]
    UnknownTagType(u8),
    /// `Tag::data` is longer than FLV's 24-bit `DataSize` field can represent (16,777,215 bytes).
    #[error("tag data of {0} bytes exceeds FLV's 24-bit DataSize field (max 16,777,215)")]
    TagDataTooLarge(usize),
    /// `Muxer::write_tag` was called before `Muxer::write_header`.
    #[error("write_tag called before write_header")]
    HeaderNotWritten,
}
