//! Map [`CodecKind`] to VA-API profile/entrypoint for encode.

use crate::EncodeError;
use mediaway_common::CodecKind;

/// VA-API profile for a Mediaway video codec.
///
/// # Errors
///
/// Returns [`EncodeError::Unsupported`] for anything but [`CodecKind::H264`]/[`CodecKind::Vp9`]
/// — HEVC / AV1 are deferred (see `docs/roadmap.md`).
pub(super) const fn video_profile(
    codec: CodecKind,
) -> Result<cros_libva::VAProfile::Type, EncodeError> {
    match codec {
        // Constrained Baseline: widest hardware/driver support, matches this stage's
        // CAVLC-only, no-B-frames, single-slice-per-frame scope (ADR-0001).
        CodecKind::H264 => Ok(cros_libva::VAProfile::VAProfileH264ConstrainedBaseline),
        // Profile 0 (8-bit 4:2:0) — this crate's only supported VP9 chroma/bit-depth convention
        // (adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md § Scope).
        CodecKind::Vp9 => Ok(cros_libva::VAProfile::VAProfileVP9Profile0),
        _ => Err(EncodeError::Unsupported),
    }
}

/// Whether this crate's video encode path accepts `codec`.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264 | CodecKind::Vp9)
}
