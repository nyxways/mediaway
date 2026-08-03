//! Map [`CodecKind`] to Media Foundation subtypes for decode.

#![allow(unsafe_code)]

use crate::DecodeError;
use mediaway_common::CodecKind;
use windows::Win32::Media::MediaFoundation::{
    MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_VP90,
};
use windows::core::GUID;

/// MF compressed input subtype for a Mediaway video codec.
pub(super) const fn video_subtype(codec: CodecKind) -> Result<GUID, DecodeError> {
    match codec {
        CodecKind::H264 => Ok(MFVideoFormat_H264),
        CodecKind::Hevc => Ok(MFVideoFormat_HEVC),
        CodecKind::Av1 => Ok(MFVideoFormat_AV1),
        CodecKind::Vp9 => Ok(MFVideoFormat_VP90),
        _ => Err(DecodeError::Unsupported),
    }
}

#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(
        codec,
        CodecKind::H264 | CodecKind::Hevc | CodecKind::Av1 | CodecKind::Vp9
    )
}
