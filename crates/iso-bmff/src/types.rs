//! Freestanding track/Sample types (no Mediaway dependency).

#![forbid(unsafe_code)]

pub use bytes::Bytes;

/// Integer rational timebase (`num / den` seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational {
    /// Numerator.
    pub num: u64,
    /// Denominator (non-zero for valid timebases).
    pub den: u32,
}

impl Rational {
    /// Construct a rational.
    #[must_use]
    pub const fn new(num: u64, den: u32) -> Self {
        Self { num, den }
    }
}

/// Codec identity for a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Codec {
    /// H.264 / AVC.
    H264,
    /// HEVC / H.265.
    Hevc,
    /// AV1.
    Av1,
    /// VP9.
    Vp9,
    /// AAC.
    Aac,
    /// Opus.
    Opus,
    /// `WebVTT`.
    WebVtt,
    /// Tx3g.
    Tx3g,
}

/// Track / stream description from `moov` (or mux registration).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// 0-based track id.
    pub id: u32,
    /// Codec.
    pub codec: Codec,
    /// Media timebase.
    pub time_base: Rational,
    /// Video width (0 for audio).
    pub width: u32,
    /// Video height (0 for audio).
    pub height: u32,
    /// Codec config (e.g. AVCC).
    pub extra_data: Bytes,
}

/// One compressed sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    /// Track id.
    pub stream_id: u32,
    /// Presentation timestamp (media timescale; may be negative after edit-list remap).
    pub pts: i64,
    /// Decode timestamp (media timescale; may be negative after edit-list remap).
    pub dts: i64,
    /// Duration.
    pub duration: u64,
    /// Sync / keyframe.
    pub is_keyframe: bool,
    /// Outside the active edit window (decode dependency / padding). Decoders may skip.
    pub is_discard: bool,
    /// Payload bytes.
    pub payload: Bytes,
}
