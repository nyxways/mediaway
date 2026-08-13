//! Map [`CodecKind`] to an `AMediaCodec` MIME type for encode.

use crate::EncodeError;
use mediaway_common::CodecKind;

/// `AMediaCodec` MIME type for a Mediaway video codec, this crate's Stage 1 scope only.
///
/// # Errors
///
/// Returns [`EncodeError::Unsupported`] for anything but [`CodecKind::H264`] — HEVC / AV1 /
/// VP9 are deferred (see `docs/roadmap.md`).
pub(super) const fn mime_type(codec: CodecKind) -> Result<&'static str, EncodeError> {
    match codec {
        CodecKind::H264 => Ok("video/avc"),
        _ => Err(EncodeError::Unsupported),
    }
}

/// Whether this crate's video encode path accepts `codec`.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264)
}
