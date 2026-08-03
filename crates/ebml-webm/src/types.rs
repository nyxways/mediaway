//! Freestanding `WebM` track/frame types (no Mediaway dependency).

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

/// Track metadata from `Tracks\TrackEntry` (v1 subset — see crate `adr/0001`).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackInfo {
    /// `TrackNumber` — referenced by `SimpleBlock`'s track number field.
    pub track_number: u64,
    /// Raw `TrackType` (1 = video, 2 = audio; other values pass through
    /// unmapped in v1).
    pub track_type: u8,
    /// Raw `WebM` `CodecID` string (e.g. `"V_VP9"`, `"A_OPUS"`). Mapping to a
    /// Mediaway `CodecKind` is the facade's job, not this crate's.
    pub codec_id: String,
    /// `TrackEntry\CodecPrivate` (codec-specific init data, e.g. `OpusHead`).
    /// `None` when absent (VP8/VP9 without config, legacy files).
    pub codec_private: Option<Bytes>,
    /// Video width in pixels (`0` for non-video tracks or if absent).
    pub width: u32,
    /// Video height in pixels (`0` for non-video tracks or if absent).
    pub height: u32,
    /// `Audio\SamplingFrequency` in Hz (spec default `8000.0` when absent; `0.0`
    /// only ever appears if a file explicitly encodes it, which spec forbids).
    pub sample_rate: f64,
    /// `Audio\Channels` (spec default `1` when absent).
    pub channels: u32,
}

impl TrackInfo {
    /// `TrackType == 1` (video), per the Matroska/`WebM` `TrackType` enum.
    #[must_use]
    pub const fn is_video(&self) -> bool {
        self.track_type == 1
    }
}

/// One demuxed `SimpleBlock`/`Block` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Owning track's `TrackNumber`.
    pub track_number: u64,
    /// Absolute timecode in `TimecodeScale` ticks (`Cluster` timecode +
    /// the block's signed relative timecode). Laced sub-frames within one
    /// block share this same timecode — Matroska lacing does not encode a
    /// distinct timecode per sub-frame (a real spec property, not a gap here).
    pub timecode: i64,
    /// Keyframe flag: `SimpleBlock`'s own flag bit, or (for a `BlockGroup`'s
    /// `Block`) the *absence* of a sibling `ReferenceBlock`.
    pub is_keyframe: bool,
    /// `BlockGroup\BlockDuration` in `TimecodeScale` ticks, if present
    /// (`None` for a bare `SimpleBlock`, which carries no duration).
    pub duration_ticks: Option<u64>,
    /// Frame payload bytes.
    pub payload: Bytes,
}

/// One `Segment\Cues\CuePoint` entry — informational seek index; this crate
/// does no seeking itself (sans-io: I/O and seeking belong to the adapter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CuePoint {
    /// `CueTime` in `TimecodeScale` ticks.
    pub time_ticks: u64,
    /// `CueClusterPosition` — byte offset from `Segment`'s data start.
    pub cluster_position: u64,
}

/// One `Segment\SeekHead\Seek` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeekEntry {
    /// The referenced element's raw ID (same representation as [`crate::vint::decode_id`]).
    pub id: u32,
    /// Byte offset from `Segment`'s data start.
    pub position: u64,
}
