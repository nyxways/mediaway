//! Map [`CodecKind`] to a `VideoToolbox` `CMVideoCodecType` for encode.

use crate::EncodeError;
use mediaway_common::CodecKind;
use objc2_core_media::CMVideoCodecType;

/// `VideoToolbox` codec type for a Mediaway video codec.
///
/// # Errors
///
/// Returns [`EncodeError::Unsupported`] for anything but [`CodecKind::H264`]/[`CodecKind::Hevc`]
/// — VP9/AV1 have no `VideoToolbox` compression API at all (confirmed: zero
/// `kVTProfileLevel_{VP9,AV1}` constants anywhere in the generated `objc2-video-toolbox`
/// bindings), a permanent platform gap rather than a deferred stage — see
/// `adr/apple/0002-videotoolbox-hevc-encode.md`.
pub(super) const fn codec_type(codec: CodecKind) -> Result<CMVideoCodecType, EncodeError> {
    match codec {
        CodecKind::H264 => Ok(objc2_core_media::kCMVideoCodecType_H264),
        CodecKind::Hevc => Ok(objc2_core_media::kCMVideoCodecType_HEVC),
        _ => Err(EncodeError::Unsupported),
    }
}

/// Whether this crate's video encode path accepts `codec`.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264 | CodecKind::Hevc)
}
