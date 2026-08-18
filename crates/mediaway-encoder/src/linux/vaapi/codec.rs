//! Map [`CodecKind`] to VA-API profile/entrypoint for encode.

use crate::EncodeError;
use mediaway_common::CodecKind;

/// VA-API profile for a Mediaway video codec.
///
/// # Errors
///
/// Returns [`EncodeError::Unsupported`] for anything but [`CodecKind::H264`]/[`CodecKind::Hevc`]
/// — AV1 / VP9 are deferred (see `docs/roadmap.md`).
pub(super) const fn video_profile(
    codec: CodecKind,
) -> Result<cros_libva::VAProfile::Type, EncodeError> {
    match codec {
        // Constrained Baseline: widest hardware/driver support, matches this stage's
        // CAVLC-only, no-B-frames, single-slice-per-frame scope (ADR-0001).
        CodecKind::H264 => Ok(cros_libva::VAProfile::VAProfileH264ConstrainedBaseline),
        // HEVC Main profile — ADR-0003 (`adr/linux/0003-vaapi-hevc-p-frame-gop.md`). Confirmed
        // against real WSL2 bindgen output (this ADR's own Addendum): same reference shape this
        // crate's existing H.264 profile constant already uses.
        CodecKind::Hevc => Ok(cros_libva::VAProfile::VAProfileHEVCMain),
        _ => Err(EncodeError::Unsupported),
    }
}

/// Whether this crate's H.264 video encode path ([`super::video::VaapiVideoEncoder`]) accepts
/// `codec`.
///
/// Deliberately narrower than [`video_profile`]: HEVC (ADR-0003) is dispatched by
/// `VaapiVideoSession::open` (`mod.rs`) to a wholly separate concrete type
/// ([`super::hevc::VaapiHevcVideoEncoder`]), which checks its own codec equality directly in its
/// own `validate()` rather than sharing this predicate — widening this function to accept both
/// codecs would make `video.rs::validate` (H.264-only) silently pass an HEVC-tagged config for
/// no benefit, since nothing downstream needs the wider check.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264)
}
