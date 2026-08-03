//! Freestanding FLV tag types (no Mediaway dependency).

#![forbid(unsafe_code)]

use bytes::Bytes;

/// FLV tag type (`TagType` byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TagType {
    /// Audio tag (8).
    Audio,
    /// Video tag (9).
    Video,
    /// Script data / AMF metadata tag (18).
    ScriptData,
}

impl TagType {
    pub(crate) const fn value(self) -> u8 {
        match self {
            Self::Audio => 8,
            Self::Video => 9,
            Self::ScriptData => 18,
        }
    }

    pub(crate) const fn from_value(value: u8) -> Option<Self> {
        match value {
            8 => Some(Self::Audio),
            9 => Some(Self::Video),
            18 => Some(Self::ScriptData),
            _ => None,
        }
    }
}

/// One FLV tag.
///
/// `data` is the already-formatted tag payload (e.g. an AAC/AVC sub-header byte
/// or two followed by codec data) — this crate frames FLV's tag/header/trailer
/// structure only; it does not interpret or build the codec-specific sub-framing
/// inside `data` (AudioTagHeader/VideoTagHeader), the same "frame, don't encode"
/// boundary as this workspace's `adts-core`/`mpeg-audio`/`ogg` crates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Tag type.
    pub tag_type: TagType,
    /// 32-bit millisecond timestamp (FLV's split 24-bit + 8-bit-extended field,
    /// combined here into one value).
    pub timestamp_ms: u32,
    /// Already-formatted tag payload.
    pub data: Bytes,
}
