//! High-level **auto** encode types (path class, fallback policy, config).
//!
//! The session lives in the platform crate to avoid a facade↔backend dependency
//! cycle. On Windows: [`mediaway_encoder_windows::auto::AutoVideoEncoder::open`].
//!
//! A future `mediaway-codec` umbrella may re-export platform constructors.

#![forbid(unsafe_code)]

use crate::video::{RateControlConfig, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{CodecKind, ColorRange, GpuDeviceHandle, PixelFormat, Rational};

/// How pixels reached the encoder (benchmark / caveat labels).
///
/// A **totally ordered**, cheapest-first cost tier. Also doubles as
/// [`AutoVideoEncodeConfig::max_path_class`]'s ceiling type: a caller states the *worst*
/// tier they'll accept, not a set, because tolerance nests monotonically (anyone
/// willing to accept [`Self::Readback`] is also willing to accept the strictly-cheaper
/// [`Self::CpuUpload`]), so no real policy skips a middle tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum EncodePathClass {
    /// GPU handle accepted with no extra device copy / readback.
    ZeroCopy,
    /// GPU→GPU copy or cross-API share blit (still no CPU round-trip).
    GpuCopy,
    /// CPU planes uploaded into a HW encoder.
    CpuUpload,
    /// GPU→CPU readback then encode (costly).
    Readback,
    /// Permissive pure-Rust software encoder (`mediaway-sw`).
    Software,
}

impl EncodePathClass {
    /// Stable short label for logs and benches (`zc` / `copy` / `upload` / `readback` / `sw`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ZeroCopy => "zc",
            Self::GpuCopy => "copy",
            Self::CpuUpload => "upload",
            Self::Readback => "readback",
            Self::Software => "sw",
        }
    }
}

/// A concrete execution path capable of encoding video.
///
/// [`Self::Os`] is one neutral tag shared across every platform even though its
/// implementation is completely different per OS (Media Foundation on Windows, VA-API
/// on Linux, `VideoToolbox` on macOS/iOS — see the platform crate for which one). Vendor
/// SDK variants ([`Self::Nvenc`], [`Self::QuickSync`], [`Self::Amf`]) are the same
/// concept on every OS they ship on, so each gets its own cross-platform identity
/// instead — see [`BackendSelection::Auto`] for why the two families are never
/// interchangeable by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Backend {
    /// The current OS's own native media-acceleration API.
    Os,
    /// NVIDIA NVENC.
    Nvenc,
    /// Intel Quick Sync Video.
    QuickSync,
    /// AMD Advanced Media Framework.
    Amf,
    /// Pure-Rust software encoder (`mediaway-sw`).
    Software,
}

/// How to pick the encode backend for a session.
///
/// Orthogonal to [`AutoVideoEncodeConfig::max_path_class`], which governs how expensive
/// the chosen backend's data path is allowed to be, independent of which backend that
/// is. See [ADR-0004](../../adr/0004-backend-preference.md) and wiki
/// `encode/backend-preference.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BackendSelection {
    /// Resolve within [`Backend::Os`]'s own path-class chain (ZC → `GpuCopy` →
    /// `CpuUpload`, gated by `max_path_class`), falling back to [`Backend::Software`]
    /// only if `max_path_class` allows it. **Never** resolves to a vendor SDK: the same
    /// silicon usually backs both the OS-native path and a vendor's own SDK, so the
    /// vendor path is not automatically faster, and picking it without being asked
    /// would be a surprise (different feature set, different bugs, different
    /// driver/licensing requirements) — opt in via [`Self::AutoHardwareOnly`] or
    /// [`Self::Explicit`] instead.
    #[default]
    Auto,
    /// Resolve to whichever hardware-capable backend is best on this machine —
    /// [`Backend::Os`] or a vendor SDK — but never [`Backend::Software`], regardless of
    /// `max_path_class`. Distinct from `Auto` (stays within `Os`) and from `Explicit`
    /// (pins one named backend, no ranking). Intended for a "performance" preset or
    /// benchmarking.
    AutoHardwareOnly,
    /// Use exactly this backend, or fail — no fallback, no ranking.
    Explicit(Backend),
}

/// Config for platform auto encode constructors (e.g. Windows `AutoVideoEncoder::open`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoVideoEncodeConfig {
    /// Output codec.
    pub codec: CodecKind,
    /// Encoded width (caller supplies; no baked-in “1080p” preset).
    pub width: u32,
    /// Encoded height.
    pub height: u32,
    /// Timestamp timebase.
    pub time_base: Rational,
    /// Target bitrate in bits per second (`0` = backend default).
    pub bitrate_bps: u32,
    /// Hint when a CPU upload path is selected.
    pub pixel_format: PixelFormat,
    /// YUV sample range of `pixel_format`'s input bytes, forwarded straight to
    /// [`VideoEncoderConfig::color_range`] — see its docs for the capability-gated fallback
    /// contract. Defaults to [`ColorRange::Video`].
    pub color_range: ColorRange,
    /// Worst [`EncodePathClass`] this session will accept, independent of `backend`.
    /// Defaults to [`EncodePathClass::CpuUpload`] (Zero-Copy / GPU-copy / CPU-upload
    /// all allowed; Readback / Software require deliberately raising this).
    pub max_path_class: EncodePathClass,
    /// GPU device handle for Zero-Copy open (`None` = unset).
    pub gpu_device: Option<GpuDeviceHandle>,
    /// Which backend to use — defaults to [`BackendSelection::Auto`].
    pub backend: BackendSelection,
    /// Frames between forced IDR refreshes, forwarded straight to
    /// [`VideoEncoderConfig::gop_size`] — see its docs for the capability-gated
    /// fallback contract. Defaults to `1` (IDR-only, byte-identical to every
    /// existing caller).
    ///
    /// **Not yet honored by any backend `AutoEncoder::open` can currently
    /// auto-select on Windows or Linux** — today only the standalone
    /// `mediaway-encoder::vulkan` H.264/HEVC encoders read this field, and the
    /// `mediaway::platform::AutoEncoder` facade never resolves to them (Vulkan
    /// Video isn't part of [`BackendSelection`] yet). Setting this through the
    /// facade is a forward-compatible no-op until that wiring lands; open the
    /// Vulkan encoder directly today for a working GOP/CBR session.
    pub gop_size: u32,
    /// CBR-style rate control request, forwarded straight to
    /// [`VideoEncoderConfig::rate_control`]. `None` (default) keeps
    /// fixed-QP encoding. Same **not yet honored via auto-select** caveat as
    /// [`Self::gop_size`] applies here too.
    pub rate_control: Option<RateControlConfig>,
}

impl AutoVideoEncodeConfig {
    /// Explicit size and codec — resolution comes from the app, not a named preset.
    #[must_use]
    pub const fn new(codec: CodecKind, width: u32, height: u32, time_base: Rational) -> Self {
        Self {
            codec,
            width,
            height,
            time_base,
            bitrate_bps: 0,
            pixel_format: PixelFormat::Nv12,
            color_range: ColorRange::Video,
            max_path_class: EncodePathClass::CpuUpload,
            gpu_device: None,
            backend: BackendSelection::Auto,
            gop_size: 1,
            rate_control: None,
        }
    }

    /// Build a low-level [`VideoEncoderConfig`] for a concrete input preference.
    #[must_use]
    pub const fn to_low_level(
        &self,
        input: VideoInputPreference,
        gpu_device: Option<GpuDeviceHandle>,
    ) -> VideoEncoderConfig {
        VideoEncoderConfig {
            codec: self.codec,
            width: self.width,
            height: self.height,
            time_base: self.time_base,
            bitrate_bps: self.bitrate_bps,
            pixel_format: self.pixel_format,
            color_range: self.color_range,
            input,
            gpu_device,
            gop_size: self.gop_size,
            rate_control: self.rate_control,
            intra_refresh_period: None,
        }
    }
}

#[cfg(test)]
#[cfg(feature = "video")]
#[path = "auto_tests.rs"]
mod tests;
