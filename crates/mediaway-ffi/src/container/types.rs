//! C ABI struct types mirroring `mediaway_common` types.
//!
//! Field layouts and ownership are decided in `adr/0001-mp4-mux-demux-c-abi.md` §5/§6.
//!
//! `MediawayRational`/`MediawayCodecKind` are type aliases into `common::types`
//! rather than defined locally (`docs/adr/0015-common-ffi-unification.md`) — the
//! C-facing type name (`mediaway_rational_t`/`mediaway_codec_kind_t`, this crate's
//! `include/mediaway/container.h`) is unaffected by where the Rust definition lives.
//! A type alias, not a `pub use` re-export, so `cbindgen` can resolve it
//! (`docs/adr/0016-cbindgen-ffi-headers.md`).

/// Codec kind — see `common::types::CodecKind`.
pub type MediawayCodecKind = crate::common::types::CodecKind;
/// Rational timebase (`num / den`, seconds) — see `common::types::Rational`.
pub type MediawayRational = crate::common::types::Rational;

/// Which container format [`crate::container::mediaway_muxer_create_for_format`]/
/// [`crate::container::mediaway_demuxer_create_for_format`] open.
///
/// Only formats sharing MP4's multi-track, typestated (`Open`→`Live`)
/// `add_video_track`/`add_audio_track`/`begin`/`push_packet`/`poll_bytes`/`flush` shape are
/// listed here — Ogg/ADTS (single implicit stream, no track registration) get their own
/// dedicated handle types (`mediaway_ogg_muxer_t`/`mediaway_adts_muxer_t`); FLV/MPEG-TS/MP3/
/// WAV have genuinely incompatible method shapes (see `adr/0003-multi-format-c-abi.md` §
/// Deferred) and are not reachable through this enum at all yet.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayContainerFormat {
    /// ISOBMFF / fragmented MP4 (`mediaway_container::mp4`) — the only format
    /// [`crate::container::mediaway_muxer_create`]/[`crate::container::mediaway_demuxer_create`]
    /// (no `_for_format` suffix) ever open.
    Mp4 = 0,
    /// `WebM` (`mediaway_container::webm`).
    Webm = 1,
}

/// One elementary stream registered in [`crate::container::mediaway_ts_muxer_create`]'s PMT.
///
/// Input to muxer construction only — `mediaway_container::ts::Muxer::new` takes the full
/// stream list upfront (no `add_track` after construction, unlike every other mux handle in
/// this crate); see `adr/0006-mpeg-ts-c-abi.md`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayTsElementaryStream {
    /// Transport Stream PID (13 bits, `2..=0x1FFF` — `0`/`1` are reserved for PAT/CAT).
    pub pid: u16,
    /// Codec — must be one of `H264`/`Hevc`/`Aac`/`Mp3` (the only `StreamType` mappings this
    /// facade has); any other value fails muxer construction.
    pub codec: MediawayCodecKind,
}

/// MPEG audio version — see `mpeg_audio::MpegVersion`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayMpegVersion {
    /// MPEG Version 1 (44100/48000/32000 Hz family).
    Mpeg1 = 0,
    /// MPEG Version 2 (22050/24000/16000 Hz family).
    Mpeg2 = 1,
    /// MPEG Version 2.5 (11025/12000/8000 Hz family, unofficial low-rate extension).
    Mpeg25 = 2,
}

/// MPEG audio channel mode — see `mpeg_audio::ChannelMode`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayChannelMode {
    /// Stereo.
    Stereo = 0,
    /// Joint stereo (intensity/MS).
    JointStereo = 1,
    /// Dual mono (two independent channels).
    DualChannel = 2,
    /// Mono.
    Mono = 3,
}

/// Fixed Layer III frame header for [`crate::container::mediaway_mp3_muxer_create`] — see
/// `mpeg_audio::FrameHeader`.
///
/// Bitrate/sample-rate/channel mode stay constant for the mux session's lifetime, matching
/// real Layer III streams this facade targets (`adr/0007-mp3-c-abi.md`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayMp3FrameHeader {
    /// MPEG version.
    pub version: MediawayMpegVersion,
    /// Bitrate in kbps — must be one of the 14 standard values for `version` (Layer III).
    pub bitrate_kbps: u16,
    /// Sample rate — must be one of the 3 standard rates for `version`.
    pub sample_rate: u32,
    /// Channel mode.
    pub channel_mode: MediawayChannelMode,
}

/// PCM sample encoding — see `riff_wave_core::SampleFormat`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayWavSampleFormat {
    /// Integer PCM (`wFormatTag` 1).
    Pcm = 0,
    /// IEEE float PCM (`wFormatTag` 3).
    Float = 1,
}

/// Explicit WAVE format for [`crate::container::mediaway_wav_muxer_create_with_format`] —
/// see `riff_wave_core::WaveFormat`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayWaveFormat {
    /// Sample encoding.
    pub sample_format: MediawayWavSampleFormat,
    /// Channel count.
    pub channels: u16,
    /// Samples per second.
    pub sample_rate: u32,
    /// Bits per sample (e.g. 16, 24, 32).
    pub bits_per_sample: u16,
}

/// Input to [`crate::container::mediaway_muxer_add_video_track`] — caller-owned, valid for the call only.
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

/// Input to [`crate::container::mediaway_muxer_add_audio_track`] — caller-owned, valid for the call only.
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

/// Input to [`crate::container::mediaway_muxer_push_packet`] — borrowed view, valid for the call only,
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

/// Output of [`crate::container::mediaway_demuxer_poll_packet`] — owned; release with
/// [`crate::container::mediaway_packet_free`].
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

/// Output of [`crate::container::mediaway_demuxer_stream_at`] — owned `extra_data`; release with
/// [`crate::container::mediaway_stream_info_free`].
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
