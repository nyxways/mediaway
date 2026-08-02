//! `Muxer::write_metadata`'s minimal `onMetaData` payload.

#![forbid(unsafe_code)]

/// Minimal `onMetaData` fields — the handful of properties FLV/RTMP publishers commonly send.
///
/// Not a general AMF value tree (see `adr/0001-rtmp-freestanding-core.md` § 3): add fields
/// here only as real callers need them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OnMetaData {
    /// Video width in pixels, if a video track is present.
    pub width: Option<f64>,
    /// Video height in pixels, if a video track is present.
    pub height: Option<f64>,
    /// Video frame rate (frames/second), if known.
    pub framerate: Option<f64>,
    /// FLV `VideoCodecID` value (e.g. `7.0` = AVC), if a video track is present.
    pub videocodecid: Option<f64>,
    /// FLV `AudioCodecID` value (e.g. `10.0` = AAC), if an audio track is present.
    pub audiocodecid: Option<f64>,
}
