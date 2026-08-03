//! Freestanding MPEG-TS types (no Mediaway dependency).

#![forbid(unsafe_code)]

use bytes::Bytes;

/// PMT `stream_type` byte (ISO/IEC 13818-1 Table 2-34 subset — the codecs this
/// workspace already supports elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamType {
    /// H.264 / AVC video (`0x1B`).
    H264,
    /// HEVC / H.265 video (`0x24`).
    Hevc,
    /// ADTS AAC audio (`0x0F`).
    Aac,
    /// MPEG-1/2 Layer I/II/III ("MP3") audio (`0x03`).
    Mp3,
}

impl StreamType {
    pub(crate) const fn value(self) -> u8 {
        match self {
            Self::H264 => 0x1B,
            Self::Hevc => 0x24,
            Self::Aac => 0x0F,
            Self::Mp3 => 0x03,
        }
    }

    pub(crate) const fn from_value(value: u8) -> Option<Self> {
        match value {
            0x1B => Some(Self::H264),
            0x24 => Some(Self::Hevc),
            0x0F => Some(Self::Aac),
            0x03 => Some(Self::Mp3),
            _ => None,
        }
    }
}

/// One elementary stream registered in the PMT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementaryStream {
    /// Transport Stream PID (13 bits — must be `<= 0x1FFF`, and not `0`/`1` which
    /// are reserved for PAT/CAT).
    pub pid: u16,
    /// Stream type.
    pub stream_type: StreamType,
}

/// One demuxed, fully reassembled elementary-stream access unit (one PES packet's
/// payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessUnit {
    /// PID this access unit was carried on.
    pub pid: u16,
    /// Already-encoded elementary-stream payload (e.g. Annex-B H.264 NAL units,
    /// or an ADTS AAC frame) — this crate frames PES/TS structure only, the same
    /// "frame, don't encode" boundary as this workspace's `adts-core`/`mpeg-audio`/
    /// `ogg`/`flv-core` crates.
    pub data: Bytes,
    /// Presentation timestamp, 90 kHz clock (33-bit range).
    pub pts_90k: u64,
    /// Decode timestamp, 90 kHz clock, if present (`None` means DTS == PTS).
    pub dts_90k: Option<u64>,
    /// Set if the TS packet carrying the start of this access unit had
    /// `random_access_indicator` set (typically: this access unit starts a
    /// keyframe / is safely seekable to).
    pub random_access: bool,
}
