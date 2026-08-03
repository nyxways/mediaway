//! `Rational`/`CodecKind` `#[repr(C)]` value-type mirrors, shared by every `mediaway-*-ffi`
//! crate.
//!
//! Named without the `Mediaway` prefix used by each consuming crate's local C type —
//! each `mediaway-*-ffi` crate re-exports these under its own existing alias (e.g.
//! `pub use mediaway_common_ffi::types::{Rational as MediawayRational};`), so the
//! C-facing type name at each crate's ABI boundary is unaffected by this crate existing.
//!
//! Moved here from `mediaway-container-ffi` (the first `-ffi` crate; taken as the source of
//! truth) after confirming `mediaway-pipeline-ffi`'s independently-transcribed copy was
//! field-identical (`docs/adr/0015-common-ffi-unification.md`). `mediaway-device-ffi` never
//! defined a codec-kind enum (no codec concept), and re-exports only [`Rational`] from here.

use mediaway_common::{CodecKind as CommonCodecKind, Rational as CommonRational};

/// Rational timebase (`num / den`, seconds) — mirrors `mediaway_common::Rational`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rational {
    /// Numerator (timestamp units).
    pub num: u64,
    /// Denominator (timebase / timescale). Must be non-zero.
    pub den: u32,
}

impl From<Rational> for CommonRational {
    fn from(r: Rational) -> Self {
        Self::new(r.num, r.den)
    }
}

impl From<CommonRational> for Rational {
    fn from(r: CommonRational) -> Self {
        Self {
            num: r.num,
            den: r.den,
        }
    }
}

/// Codec kind — mirrors `mediaway_common::CodecKind` 1:1.
///
/// Pre-1.0: values may be renumbered; do not persist these across builds.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Uncompressed / raw video.
    RawVideo = 10,
    /// Uncompressed / raw PCM audio.
    RawAudio = 11,
}

impl From<CodecKind> for CommonCodecKind {
    fn from(codec: CodecKind) -> Self {
        match codec {
            CodecKind::H264 => Self::H264,
            CodecKind::Hevc => Self::Hevc,
            CodecKind::Av1 => Self::Av1,
            CodecKind::Vp9 => Self::Vp9,
            CodecKind::Aac => Self::Aac,
            CodecKind::Opus => Self::Opus,
            CodecKind::Mp3 => Self::Mp3,
            CodecKind::Vorbis => Self::Vorbis,
            CodecKind::WebVtt => Self::WebVtt,
            CodecKind::Tx3g => Self::Tx3g,
            CodecKind::RawVideo => Self::RawVideo,
            CodecKind::RawAudio => Self::RawAudio,
        }
    }
}

impl From<CommonCodecKind> for CodecKind {
    fn from(codec: CommonCodecKind) -> Self {
        match codec {
            CommonCodecKind::H264 => Self::H264,
            CommonCodecKind::Hevc => Self::Hevc,
            CommonCodecKind::Av1 => Self::Av1,
            CommonCodecKind::Vp9 => Self::Vp9,
            CommonCodecKind::Aac => Self::Aac,
            CommonCodecKind::Opus => Self::Opus,
            CommonCodecKind::Mp3 => Self::Mp3,
            CommonCodecKind::Vorbis => Self::Vorbis,
            CommonCodecKind::WebVtt => Self::WebVtt,
            CommonCodecKind::Tx3g => Self::Tx3g,
            CommonCodecKind::RawVideo => Self::RawVideo,
            CommonCodecKind::RawAudio => Self::RawAudio,
        }
    }
}
