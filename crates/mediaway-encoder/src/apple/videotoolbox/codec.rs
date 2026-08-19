//! Map [`CodecKind`] to a `VideoToolbox` `CMVideoCodecType` for encode.

use crate::EncodeError;
use mediaway_common::CodecKind;
use objc2_core_media::CMVideoCodecType;

/// `VideoToolbox` codec type for a Mediaway video codec.
///
/// # Errors
///
/// Returns [`EncodeError::Unsupported`] for anything but [`CodecKind::H264`]/[`CodecKind::Hevc`]/
/// the six `CodecKind::ProRes*` variants — VP9/AV1 have no `VideoToolbox` compression API at all
/// (confirmed: zero `kVTProfileLevel_{VP9,AV1}` constants anywhere in the generated
/// `objc2-video-toolbox` bindings), a permanent platform gap rather than a deferred stage — see
/// `adr/apple/0002-videotoolbox-hevc-encode.md`. `ProRes` RAW/RAW HQ are a **separate** permanent
/// gap: zero references to either exist anywhere in `VTCompressionSession`/
/// `VTCompressionProperties` (RAW is camera-capture-produced only, never a general encode
/// target) — see `adr/apple/0006-videotoolbox-prores-encode.md`.
pub(super) const fn codec_type(codec: CodecKind) -> Result<CMVideoCodecType, EncodeError> {
    match codec {
        CodecKind::H264 => Ok(objc2_core_media::kCMVideoCodecType_H264),
        CodecKind::Hevc => Ok(objc2_core_media::kCMVideoCodecType_HEVC),
        CodecKind::ProRes422Proxy => Ok(objc2_core_media::kCMVideoCodecType_AppleProRes422Proxy),
        CodecKind::ProRes422Lt => Ok(objc2_core_media::kCMVideoCodecType_AppleProRes422LT),
        CodecKind::ProRes422 => Ok(objc2_core_media::kCMVideoCodecType_AppleProRes422),
        CodecKind::ProRes422Hq => Ok(objc2_core_media::kCMVideoCodecType_AppleProRes422HQ),
        CodecKind::ProRes4444 => Ok(objc2_core_media::kCMVideoCodecType_AppleProRes4444),
        CodecKind::ProRes4444Xq => Ok(objc2_core_media::kCMVideoCodecType_AppleProRes4444XQ),
        _ => Err(EncodeError::Unsupported),
    }
}

/// Whether this crate's video encode path accepts `codec`.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(
        codec,
        CodecKind::H264
            | CodecKind::Hevc
            | CodecKind::ProRes422Proxy
            | CodecKind::ProRes422Lt
            | CodecKind::ProRes422
            | CodecKind::ProRes422Hq
            | CodecKind::ProRes4444
            | CodecKind::ProRes4444Xq
    )
}

/// Whether `codec` is one of the six supported `ProRes` profiles — all-intra, no
/// `ProfileLevel`/`MaxKeyFrameInterval`/`AverageBitRate` properties apply (see
/// `configure_properties` in `video.rs` and `adr/apple/0006-videotoolbox-prores-encode.md`).
#[must_use]
pub(super) const fn is_prores(codec: CodecKind) -> bool {
    matches!(
        codec,
        CodecKind::ProRes422Proxy
            | CodecKind::ProRes422Lt
            | CodecKind::ProRes422
            | CodecKind::ProRes422Hq
            | CodecKind::ProRes4444
            | CodecKind::ProRes4444Xq
    )
}
