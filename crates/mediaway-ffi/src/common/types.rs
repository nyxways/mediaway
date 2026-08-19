//! `Rational`/`CodecKind`/`PixelFormat`/`SampleFormat`/`VideoFrameStorageKind`
//! `#[repr(C)]` value-type mirrors, shared by this crate's `container`/`device`/
//! `pipeline` modules.
//!
//! Named without the `Mediaway` prefix used by each consuming module's local C type —
//! each module re-exports these as a **type alias**, not a `pub use` re-export (e.g.
//! `pub type MediawayRational = crate::common::types::Rational;`), so the C-facing
//! type name at each module's ABI boundary is unaffected by where the definition
//! lives. The alias form (not `pub use ... as`) matters for `cbindgen`
//! (`docs/adr/0016-cbindgen-ffi-headers.md`): it cannot resolve a bare re-export to
//! its underlying `#[repr(C)]` definition, but follows a `pub type` alias correctly.
//!
//! `Rational`/`CodecKind` moved here from `mediaway-container-ffi` (the first `-ffi`
//! crate; taken as the source of truth) after confirming `mediaway-ffi`'s
//! independently-transcribed copy was field-identical
//! (`docs/adr/0015-common-ffi-unification.md`). `PixelFormat`/`SampleFormat`/
//! `VideoFrameStorageKind` moved here later from independently-duplicated copies in
//! `device::types`/`pipeline::types`
//! (`adr/common/0001-shared-header-consolidation.md`).

use mediaway_common::{
    CodecKind as CommonCodecKind, PixelFormat as CommonPixelFormat, Rational as CommonRational,
    SampleFormat as CommonSampleFormat,
};

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
    /// VP8 video.
    Vp8 = 12,
    /// Apple `ProRes` 422 Proxy video.
    ProRes422Proxy = 13,
    /// Apple `ProRes` 422 LT video.
    ProRes422Lt = 14,
    /// Apple `ProRes` 422 (standard) video.
    ProRes422 = 15,
    /// Apple `ProRes` 422 HQ video.
    ProRes422Hq = 16,
    /// Apple `ProRes` 4444 video.
    ProRes4444 = 17,
    /// Apple `ProRes` 4444 XQ video.
    ProRes4444Xq = 18,
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
            CodecKind::Vp8 => Self::Vp8,
            CodecKind::ProRes422Proxy => Self::ProRes422Proxy,
            CodecKind::ProRes422Lt => Self::ProRes422Lt,
            CodecKind::ProRes422 => Self::ProRes422,
            CodecKind::ProRes422Hq => Self::ProRes422Hq,
            CodecKind::ProRes4444 => Self::ProRes4444,
            CodecKind::ProRes4444Xq => Self::ProRes4444Xq,
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
            CommonCodecKind::Vp8 => Self::Vp8,
            CommonCodecKind::ProRes422Proxy => Self::ProRes422Proxy,
            CommonCodecKind::ProRes422Lt => Self::ProRes422Lt,
            CommonCodecKind::ProRes422 => Self::ProRes422,
            CommonCodecKind::ProRes422Hq => Self::ProRes422Hq,
            CommonCodecKind::ProRes4444 => Self::ProRes4444,
            CommonCodecKind::ProRes4444Xq => Self::ProRes4444Xq,
        }
    }
}

/// Pixel layout — mirrors `mediaway_common::PixelFormat`'s 5 variants.
///
/// Moved here from independently-defined copies in `device::types` and
/// `pipeline::types` (former `mediaway-device-ffi`/`mediaway-ffi` crates) after
/// confirming they were field-identical — same consolidation `Rational`/[`CodecKind`]
/// already went through (`docs/adr/0015-common-ffi-unification.md`). Each consuming
/// module re-exports this under its own existing `MediawayPixelFormat` alias, so the
/// C-facing type name is unaffected.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 8-bit NV12 (YUV 4:2:0 semi-planar) — common HW encode input.
    Nv12 = 0,
    /// 8-bit I420 / YUV420P.
    I420 = 1,
    /// 8-bit BGRA packed.
    Bgra8 = 2,
    /// 8-bit RGBA packed.
    Rgba8 = 3,
    /// 8-bit YUYV / YUY2 packed (YUV 4:2:2).
    Yuyv = 4,
}

impl From<PixelFormat> for CommonPixelFormat {
    fn from(format: PixelFormat) -> Self {
        match format {
            PixelFormat::Nv12 => Self::Nv12,
            PixelFormat::I420 => Self::I420,
            PixelFormat::Bgra8 => Self::Bgra8,
            PixelFormat::Rgba8 => Self::Rgba8,
            PixelFormat::Yuyv => Self::Yuyv,
        }
    }
}

impl From<CommonPixelFormat> for PixelFormat {
    // `PixelFormat` is `#[non_exhaustive]`; all variants that exist today are matched
    // by name below. No "unknown" C variant exists to fall back to, so a future
    // variant maps to the safest default (NV12) — that overlap with the `Nv12` arm's
    // own body is intentional, not a copy-paste bug.
    #[allow(clippy::match_same_arms)]
    fn from(format: CommonPixelFormat) -> Self {
        match format {
            CommonPixelFormat::Nv12 => Self::Nv12,
            CommonPixelFormat::I420 => Self::I420,
            CommonPixelFormat::Bgra8 => Self::Bgra8,
            CommonPixelFormat::Rgba8 => Self::Rgba8,
            CommonPixelFormat::Yuyv => Self::Yuyv,
            _ => Self::Nv12,
        }
    }
}

/// Audio PCM sample layout — mirrors `mediaway_common::SampleFormat`'s 3 variants.
///
/// Moved here from independently-defined copies in `device::types` and
/// `pipeline::types`, same consolidation as [`PixelFormat`] above.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// Signed 16-bit little-endian interleaved PCM.
    S16 = 0,
    /// Signed 32-bit little-endian interleaved PCM.
    S32 = 1,
    /// IEEE float32 interleaved PCM.
    F32 = 2,
}

impl From<SampleFormat> for CommonSampleFormat {
    fn from(format: SampleFormat) -> Self {
        match format {
            SampleFormat::S16 => Self::S16,
            SampleFormat::S32 => Self::S32,
            SampleFormat::F32 => Self::F32,
        }
    }
}

impl From<CommonSampleFormat> for SampleFormat {
    // `SampleFormat` is `#[non_exhaustive]`; all variants that exist today are matched
    // by name below. A future variant falls back to F32 — the format the real Windows
    // WASAPI backend already requires today, not an arbitrary choice.
    #[allow(clippy::match_same_arms)]
    fn from(format: CommonSampleFormat) -> Self {
        match format {
            CommonSampleFormat::S16 => Self::S16,
            CommonSampleFormat::S32 => Self::S32,
            CommonSampleFormat::F32 => Self::F32,
            _ => Self::F32,
        }
    }
}

/// Which of a video frame's two storage fields (CPU bytes vs. GPU handle) is valid.
///
/// FFI-layer-only discriminant — no direct `mediaway_common` counterpart to mirror
/// (unlike [`PixelFormat`]/[`SampleFormat`]). Moved here from independently-defined
/// copies in `device::types` and `pipeline::types`, same consolidation as above.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFrameStorageKind {
    /// CPU byte buffer is valid; the GPU handle field is unused/zeroed.
    Cpu = 0,
    /// The GPU handle field is valid; the CPU byte buffer is null/empty.
    Gpu = 1,
}
