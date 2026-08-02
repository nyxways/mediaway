//! C ABI struct types mirroring `mediaway_common` types.
//!
//! Field layouts and ownership are decided in `adr/0001-mp4-mux-demux-c-abi.md` §5/§6.
//!
//! `MediawayRational`/`MediawayCodecKind` are re-exported from `mediaway-common-ffi`
//! rather than defined locally (`docs/adr/0015-common-ffi-unification.md`) — the
//! C-facing type name (`mediaway_rational_t`/`mediaway_codec_kind_t`, this crate's
//! `include/mediaway/container.h`) is unaffected by where the Rust definition lives.
pub use mediaway_common_ffi::types::{
    CodecKind as MediawayCodecKind, Rational as MediawayRational,
};

/// Input to [`crate::mediaway_muxer_add_video_track`] — caller-owned, valid for the call only.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayVideoTrackInfo {
    /// Caller-assigned track id; must be unique per muxer.
    pub id: u32,
    /// Codec kind.
    pub codec: MediawayCodecKind,
    /// Timebase for timestamps on packets belonging to this track.
    pub time_base: MediawayRational,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Borrowed extra header data (e.g. AVCC), valid for the call only. Null iff
    /// `extra_data_len == 0`.
    pub extra_data: *const u8,
    /// Length of `extra_data` in bytes.
    pub extra_data_len: usize,
}

/// Input to [`crate::mediaway_muxer_add_audio_track`] — caller-owned, valid for the call only.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayAudioTrackInfo {
    /// Caller-assigned track id; must be unique per muxer.
    pub id: u32,
    /// Codec kind.
    pub codec: MediawayCodecKind,
    /// Timebase for timestamps on packets belonging to this track.
    pub time_base: MediawayRational,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Borrowed extra header data, valid for the call only. Null iff `extra_data_len == 0`.
    pub extra_data: *const u8,
    /// Length of `extra_data` in bytes.
    pub extra_data_len: usize,
}

/// Input to [`crate::mediaway_muxer_push_packet`] — borrowed view, valid for the call only,
/// no free function.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayPacketView {
    /// Stream / track id this packet belongs to.
    pub stream_id: u32,
    /// Presentation timestamp, in the track's timebase units.
    pub pts: i64,
    /// Decode timestamp, in the track's timebase units.
    pub dts: i64,
    /// Duration, in the track's timebase units.
    pub duration: u64,
    /// Whether this packet is a keyframe / random access point.
    pub is_keyframe: bool,
    /// Whether this packet is outside the active edit window.
    pub is_discard: bool,
    /// Borrowed payload bytes, valid for the call only. Null iff `payload_len == 0`.
    pub payload: *const u8,
    /// Length of `payload` in bytes.
    pub payload_len: usize,
}

/// Output of [`crate::mediaway_demuxer_poll_packet`] — owned; release with
/// [`crate::mediaway_packet_free`].
#[repr(C)]
#[derive(Debug)]
pub struct MediawayPacket {
    /// Stream / track id this packet belongs to.
    pub stream_id: u32,
    /// Presentation timestamp, in the track's timebase units.
    pub pts: i64,
    /// Decode timestamp, in the track's timebase units.
    pub dts: i64,
    /// Duration, in the track's timebase units.
    pub duration: u64,
    /// Whether this packet is a keyframe / random access point.
    pub is_keyframe: bool,
    /// Whether this packet is outside the active edit window.
    pub is_discard: bool,
    /// Owned payload bytes.
    pub payload: *mut u8,
    /// Length of `payload` in bytes.
    pub payload_len: usize,
}

/// Output of [`crate::mediaway_demuxer_stream_at`] — owned `extra_data`; release with
/// [`crate::mediaway_stream_info_free`].
#[repr(C)]
#[derive(Debug)]
pub struct MediawayStreamInfo {
    /// Track or stream index.
    pub id: u32,
    /// Codec kind.
    pub codec: MediawayCodecKind,
    /// Timebase for timestamps on packets belonging to this track.
    pub time_base: MediawayRational,
    /// Whether `width`/`height` are meaningful (video tracks only).
    pub has_geometry: bool,
    /// Width in pixels. Valid only if `has_geometry`.
    pub width: u32,
    /// Height in pixels. Valid only if `has_geometry`.
    pub height: u32,
    /// Sample rate in Hz. `0` if not applicable.
    pub sample_rate: u32,
    /// Channel count. `0` if not applicable.
    pub channels: u16,
    /// Owned extra header data.
    pub extra_data: *mut u8,
    /// Length of `extra_data` in bytes.
    pub extra_data_len: usize,
}
