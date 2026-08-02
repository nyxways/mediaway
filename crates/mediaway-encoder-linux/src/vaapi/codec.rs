//! Map [`CodecKind`] to VA-API profile/entrypoint for encode.

use mediaway_common::CodecKind;
use mediaway_encoder::EncodeError;

/// VA-API profile for a Mediaway video codec, this crate's Stage 1 scope only.
///
/// # Errors
///
/// Returns [`EncodeError::Unsupported`] for anything but [`CodecKind::H264`] — HEVC / AV1 /
/// VP9 are deferred (see `docs/roadmap.md`).
pub(super) const fn video_profile(
    codec: CodecKind,
) -> Result<cros_libva::VAProfile::Type, EncodeError> {
    match codec {
        // Constrained Baseline: widest hardware/driver support, matches this stage's
        // CAVLC-only, no-B-frames, single-slice-per-frame scope (ADR-0001).
        CodecKind::H264 => Ok(cros_libva::VAProfile::VAProfileH264ConstrainedBaseline),
        _ => Err(EncodeError::Unsupported),
    }
}

/// Whether this crate's video encode path accepts `codec`.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264)
}
