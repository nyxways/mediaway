//! Shared Mediaway types: timebase, pixel/sample formats, GPU buffer handles.
//!
//! Platform backends and encode/decode crates all depend on this crate.
//! Keep it dependency-light and `unsafe`-free.

#![forbid(unsafe_code)]

mod formats;
mod frame;
mod gpu;

pub use bytes::Bytes;
pub use formats::{PixelFormat, SampleFormat};
pub use frame::{AudioFrame, VideoFrame, VideoFrameStorage};
pub use gpu::{GpuBufferHandle, GpuDeviceHandle, NativeHandle};

/// Integer rational timebase (`num / den` seconds) for precise fractional timestamp conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational {
    /// Numerator (timestamp units).
    pub num: u64,
    /// Denominator (timebase / timescale). Must be non-zero.
    pub den: u32,
}

impl Rational {
    /// Construct a rational. Panics are forbidden in production paths — callers
    /// must validate `den != 0` before construction when input is untrusted.
    #[must_use]
    pub const fn new(num: u64, den: u32) -> Self {
        Self { num, den }
    }
}

/// Supported codec types for demuxing and muxing.
///
/// `#[repr(u8)]` with explicit discriminants, kept in lockstep with
/// `mediaway_codec_kind_t` in `crates/mediaway-ffi/include/mediaway/container.h` — this
/// type crosses the C ABI directly (`mediaway-ffi`'s `MediawayCodecKind` is a type alias to
/// this enum, not a converting wrapper), so an implicit/compiler-chosen discriminant order
/// is a real correctness bug, not just a style nit: `Vp8` was appended to the C header at
/// `= 12` when `WebM` support landed, but this enum's declaration order put it 5th
/// (discriminant `4`, sandwiched between `Vp9` and `Aac`) — every codec from `Aac` onward
/// silently carried the wrong wire value across the C ABI until these discriminants were
/// pinned explicitly to match the header.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecKind {
    /// H.264 / AVC video.
    H264 = 0,
    /// HEVC / H.265 video.
    Hevc = 1,
    /// AV1 video.
    Av1 = 2,
    /// VP9 video.
    Vp9 = 3,
    /// AAC audio.
    Aac = 4,
    /// Opus audio.
    Opus = 5,
    /// MP3 (MPEG-1/2/2.5 Layer III) audio.
    Mp3 = 6,
    /// Vorbis audio.
    Vorbis = 7,
    /// `WebVTT` subtitle.
    WebVtt = 8,
    /// Tx3g timed text subtitle.
    Tx3g = 9,
    /// Uncompressed / raw video (capture, passthrough).
    RawVideo = 10,
    /// Uncompressed / raw PCM audio — the audio analog of [`CodecKind::RawVideo`].
    /// Covers both capture/passthrough (no container) and container-framed PCM
    /// (e.g. RIFF/WAVE `data` chunk) — PCM has no encoding to distinguish either way.
    RawAudio = 11,
    /// VP8 video.
    Vp8 = 12,
}

impl CodecKind {
    /// Whether this codec kind produces/consumes video frames (has geometry).
    #[must_use]
    pub const fn is_video(self) -> bool {
        matches!(
            self,
            Self::H264 | Self::Hevc | Self::Av1 | Self::Vp9 | Self::Vp8 | Self::RawVideo
        )
    }
}

/// Pixel dimensions of a video track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VideoGeometry {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Description of a media stream/track.
///
/// `Video` and `Audio` are separate variants — rather than one shape with
/// optional fields — so a track can never claim video dimensions it doesn't
/// have, or omit dimensions it does: the invariant lives in the type, not in
/// a convention callers must remember. `Audio` is also used for subtitle
/// tracks (`WebVtt`/`Tx3g`) — there is no separate `Subtitle` variant yet;
/// `sample_rate`/`channels` are `0` there (not applicable, not "silence").
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamInfo {
    /// A video track — always carries pixel dimensions.
    Video {
        /// Track or stream index.
        id: u32,
        /// Codec kind of this track.
        codec: CodecKind,
        /// Timebase for timestamps in packets on this track.
        time_base: Rational,
        /// Pixel dimensions.
        geometry: VideoGeometry,
        /// Extra header data (e.g. AVCC / extradata).
        extra_data: Bytes,
    },
    /// A non-video track (audio, or subtitle until a dedicated variant
    /// exists) — no geometry.
    Audio {
        /// Track or stream index.
        id: u32,
        /// Codec kind of this track.
        codec: CodecKind,
        /// Timebase for timestamps in packets on this track.
        time_base: Rational,
        /// Extra header data (e.g. AVCC / extradata).
        extra_data: Bytes,
        /// Sample rate in Hz. `0` when unknown or not applicable (e.g. a
        /// subtitle track, or a source that doesn't carry this yet).
        sample_rate: u32,
        /// Channel count. `0` when unknown or not applicable.
        channels: u16,
    },
}

impl StreamInfo {
    /// Track or stream index, regardless of kind.
    #[must_use]
    pub const fn id(&self) -> u32 {
        match self {
            Self::Video { id, .. } | Self::Audio { id, .. } => *id,
        }
    }

    /// Return a copy with a different track id (e.g. renumbering before mux registration).
    #[must_use]
    pub fn with_id(self, id: u32) -> Self {
        match self {
            Self::Video {
                codec,
                time_base,
                geometry,
                extra_data,
                ..
            } => Self::Video {
                id,
                codec,
                time_base,
                geometry,
                extra_data,
            },
            Self::Audio {
                codec,
                time_base,
                extra_data,
                sample_rate,
                channels,
                ..
            } => Self::Audio {
                id,
                codec,
                time_base,
                extra_data,
                sample_rate,
                channels,
            },
        }
    }

    /// Codec kind of this track.
    #[must_use]
    pub const fn codec(&self) -> CodecKind {
        match self {
            Self::Video { codec, .. } | Self::Audio { codec, .. } => *codec,
        }
    }

    /// Timebase for timestamps in packets on this track.
    #[must_use]
    pub const fn time_base(&self) -> Rational {
        match self {
            Self::Video { time_base, .. } | Self::Audio { time_base, .. } => *time_base,
        }
    }

    /// Extra header data (e.g. AVCC / extradata).
    #[must_use]
    pub const fn extra_data(&self) -> &Bytes {
        match self {
            Self::Video { extra_data, .. } | Self::Audio { extra_data, .. } => extra_data,
        }
    }

    /// Pixel dimensions, if this is a video track.
    #[must_use]
    pub const fn geometry(&self) -> Option<VideoGeometry> {
        match self {
            Self::Video { geometry, .. } => Some(*geometry),
            Self::Audio { .. } => None,
        }
    }

    /// Sample rate in Hz, if this is a non-video track. `Some(0)` means
    /// unknown/not applicable, not "silence" — see [`Self::Audio`].
    #[must_use]
    pub const fn sample_rate(&self) -> Option<u32> {
        match self {
            Self::Audio { sample_rate, .. } => Some(*sample_rate),
            Self::Video { .. } => None,
        }
    }

    /// Channel count, if this is a non-video track. `Some(0)` means
    /// unknown/not applicable — see [`Self::Audio`].
    #[must_use]
    pub const fn channels(&self) -> Option<u16> {
        match self {
            Self::Audio { channels, .. } => Some(*channels),
            Self::Video { .. } => None,
        }
    }
}

/// Elementary compressed packet passed between demuxers, decoders, encoders, and muxers.
///
/// `payload` uses [`Bytes`] so clones are reference-counted (cheap) instead of full
/// bitstream copies on hot paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// Stream / track ID this packet belongs to.
    pub stream_id: u32,
    /// Presentation timestamp unit count (may be negative after edit-list remap).
    pub pts: i64,
    /// Decode timestamp unit count (may be negative after edit-list remap).
    pub dts: i64,
    /// Duration of the packet in timestamp units.
    pub duration: u64,
    /// Whether this packet represents a keyframe / random access point.
    pub is_keyframe: bool,
    /// Outside the active edit window (decode dependency / padding). Decoders may skip.
    pub is_discard: bool,
    /// Compressed bitstream payload bytes.
    pub payload: Bytes,
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
