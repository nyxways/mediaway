//! Map [`CodecKind`] / SPS `profile_idc` to VA-API profile candidates for decode.

use crate::DecodeError;
use mediaway_common::CodecKind;

/// VA-API profile candidates for a parsed SPS `profile_idc`, best match first.
///
/// This crate's [`super::sps::Sps::parse`] already rejects any `profile_idc` outside
/// `{66, 77}` (baseline / main — this session's scope, see
/// `adr/0001-vaapi-h264-cpu-out.md`), so the `_` arm here is unreachable in practice but kept
/// exhaustive and honest rather than assuming the caller always validates first.
///
/// # Errors
///
/// Returns [`DecodeError::Unsupported`] for any `profile_idc` this crate does not decode.
pub(super) fn h264_profile_candidates(
    profile_idc: u8,
) -> Result<Vec<cros_libva::VAProfile::Type>, DecodeError> {
    match profile_idc {
        // Baseline: most drivers only expose the Constrained Baseline VA-API profile.
        66 => Ok(vec![
            cros_libva::VAProfile::VAProfileH264ConstrainedBaseline,
        ]),
        // Main: prefer an exact match, fall back to Constrained Baseline (a strict decode
        // subset of Main for VLD purposes on drivers that do not enumerate Main separately).
        77 => Ok(vec![
            cros_libva::VAProfile::VAProfileH264Main,
            cros_libva::VAProfile::VAProfileH264ConstrainedBaseline,
        ]),
        _ => Err(DecodeError::Unsupported),
    }
}

/// VA-API profile candidates for AV1 decode — this crate's AV1 `SequenceHeader::parse` already
/// rejects any `seq_profile` other than `0` (Main — see
/// `adr/linux/0003-vaapi-av1-key-frame-decode.md` § Scope), so this always returns the single
/// Main-profile candidate.
#[must_use]
pub(super) fn av1_profile_candidates() -> Vec<cros_libva::VAProfile::Type> {
    vec![cros_libva::VAProfile::VAProfileAV1Profile0]
}

/// VA-API profile candidates for VP9 decode — this crate's VP9 `Header::parse` already rejects
/// any `Profile` other than `0` (8-bit 4:2:0 — see
/// `adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md` § Scope), so this always returns the
/// single Profile-0 candidate.
#[must_use]
pub(super) fn vp9_profile_candidates() -> Vec<cros_libva::VAProfile::Type> {
    vec![cros_libva::VAProfile::VAProfileVP9Profile0]
}

/// Whether this crate's video decode path accepts `codec`.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264 | CodecKind::Av1 | CodecKind::Vp9)
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
