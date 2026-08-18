//! Map [`CodecKind`] to an `AMediaCodec` MIME type for decode.

use crate::DecodeError;
use mediaway_common::CodecKind;

/// `AMediaCodec` MIME type for a Mediaway video codec, this crate's Stage 1 scope only.
///
/// # Errors
///
/// Returns [`DecodeError::Unsupported`] for anything but [`CodecKind::H264`] — HEVC / AV1 /
/// VP9 are deferred (see `docs/roadmap.md`).
pub(super) const fn mime_type(codec: CodecKind) -> Result<&'static str, DecodeError> {
    match codec {
        CodecKind::H264 => Ok("video/avc"),
        _ => Err(DecodeError::Unsupported),
    }
}

/// Whether this crate's video decode path accepts `codec`.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264)
}
