//! Map [`CodecKind`] to Media Foundation subtypes for encode/decode.

#![allow(unsafe_code)]

use mediaway_common::CodecKind;
use crate::EncodeError;
use windows::Win32::Media::MediaFoundation::{
    MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_VP90,
};
use windows::core::GUID;

/// MF output (encode) / input (decode) subtype for a Mediaway video codec.
pub(super) const fn video_subtype(codec: CodecKind) -> Result<GUID, EncodeError> {
    match codec {
        CodecKind::H264 => Ok(MFVideoFormat_H264),
        CodecKind::Hevc => Ok(MFVideoFormat_HEVC),
        CodecKind::Av1 => Ok(MFVideoFormat_AV1),
        CodecKind::Vp9 => Ok(MFVideoFormat_VP90),
        _ => Err(EncodeError::Unsupported),
    }
}

/// Whether this crate's video encode path accepts `codec`.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(
        codec,
        CodecKind::H264 | CodecKind::Hevc | CodecKind::Av1 | CodecKind::Vp9
    )
}
