//! Map [`CodecKind`] to VA-API profile/entrypoint for encode.

use crate::EncodeError;
use mediaway_common::CodecKind;

/// VA-API profile for a Mediaway video codec.
///
/// # Errors
///
/// Returns [`EncodeError::Unsupported`] for anything but [`CodecKind::H264`]/[`CodecKind::Hevc`]/
/// [`CodecKind::Vp9`] — AV1 is designed but blocked (see `docs/roadmap.md`).
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
        // Profile 0 (8-bit 4:2:0) — this crate's only supported VP9 chroma/bit-depth convention
        // (adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md § Scope).
        CodecKind::Vp9 => Ok(cros_libva::VAProfile::VAProfileVP9Profile0),
        _ => Err(EncodeError::Unsupported),
    }
}

/// Whether this crate's `linux::vaapi` backend encodes `codec` at all — used by `mod.rs`'s
/// dispatcher to route a config to one of its per-codec concrete encoder types
/// ([`super::video::VaapiH264Encoder`], [`super::hevc::VaapiHevcVideoEncoder`],
/// [`super::vp9::VaapiVp9Encoder`]). Each of those types checks its own codec equality directly
/// in its own `validate()` rather than delegating to this predicate — widening this function is
/// therefore safe (it only gates *routing*, never a per-codec accept/reject decision on its
/// own). AV1 is not listed: encode is designed but blocked (no concrete type exists to route to).
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264 | CodecKind::Hevc | CodecKind::Vp9)
}
