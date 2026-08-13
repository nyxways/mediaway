//! Video encode config and [`VideoEncoder`] trait.

#![forbid(unsafe_code)]

use crate::error::EncodeError;
use mediaway_common::{
    CodecKind, ColorRange, GpuDeviceHandle, Packet, PixelFormat, Rational, StreamInfo, VideoFrame,
};

/// How the caller prefers to feed frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum VideoInputPreference {
    /// Prefer GPU handles ([`mediaway_common::VideoFrameStorage::Gpu`]).
    #[default]
    ZeroCopyGpu,
    /// Accept CPU frames (may upload — backends must document cost).
    CpuUploadOk,
}

/// Parameters for opening a video encoder session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoEncoderConfig {
    /// Output codec (Stage 1 Windows: [`CodecKind::H264`]).
    pub codec: CodecKind,
    /// Encoded width.
    pub width: u32,
    /// Encoded height.
    pub height: u32,
    /// Timestamp timebase for input frames and output packets.
    pub time_base: Rational,
    /// Target bitrate in bits per second (`0` = backend default).
    pub bitrate_bps: u32,
    /// Preferred input pixel format when the backend converts.
    pub pixel_format: PixelFormat,
    /// YUV sample range of [`Self::pixel_format`]'s input bytes (irrelevant for packed RGB
    /// formats). Defaults to [`ColorRange::Video`]. A backend that cannot honor a non-default
    /// value falls back to its native default and must document that fallback on its own
    /// encoder type's rustdoc, per `caveats-and-clarity.md` — same convention as
    /// [`Self::gop_size`] / [`Self::rate_control`].
    pub color_range: ColorRange,
    /// Input path preference (Zero-Copy vs CPU upload).
    pub input: VideoInputPreference,
    /// GPU device handle when [`VideoInputPreference::ZeroCopyGpu`].
    ///
    /// Must be the device that owns submitted GPU buffers (e.g.
    /// [`GpuBufferHandle::DirectX11`](mediaway_common::GpuBufferHandle) textures).
    /// `None` = unset → Zero-Copy open fails.
    pub gpu_device: Option<GpuDeviceHandle>,
    /// Frames between forced IDR refreshes. `1` = IDR-only (every frame an
    /// independent key frame — the `Default`/`h264()`/`hevc()`/`av1()`/`vp9()`
    /// constructor value, zero behavior change for existing callers). `0` is
    /// rejected by backends that read this field (an explicit value avoids
    /// silent unbounded drift; see each backend's `open`/`EncodeError` docs)
    /// rather than treated as "infinite GOP". A backend that cannot honor a
    /// value `> 1` (no multi-slot DPB / P-frame support) falls back to
    /// IDR-only and must document that fallback on its own encoder type's
    /// rustdoc, per `caveats-and-clarity.md`.
    pub gop_size: u32,
    /// Target bitrate ceiling for CBR-style rate control. `None` keeps
    /// fixed-QP encoding (today's only behavior). `Some(_)` is a request,
    /// not a guarantee — a backend that cannot honor CBR (capability-gated)
    /// falls back to fixed-QP and must document that fallback on its own
    /// encoder type's rustdoc, per `caveats-and-clarity.md`.
    pub rate_control: Option<RateControlConfig>,
    /// Row-based intra-refresh wave period, in frames. `None` (the
    /// `Default`/`h264()`/`hevc()`/`av1()`/`vp9()` constructor value, zero
    /// behavior change for existing callers) disables it. `Some(n)` requests
    /// continuous back-to-back refresh waves of `n` frames each instead of
    /// periodic full IDR frames — every frame after the session's one
    /// startup IDR stays a P frame, with a cyclically advancing band of
    /// intra-coded rows standing in for a keyframe's bandwidth spike. Takes
    /// priority over [`Self::gop_size`]'s periodic-IDR cadence when both are
    /// set (a backend honoring intra-refresh needs an unbounded/"infinite"
    /// GOP structure, which is incompatible with periodic forced IDRs). A
    /// backend that cannot honor intra-refresh (capability-gated) falls back
    /// to its `gop_size` behavior with no error and must document that
    /// fallback on its own encoder type's rustdoc, per `caveats-and-clarity.md`.
    pub intra_refresh_period: Option<u32>,
}

/// Target bitrate + optional VBV buffer size for CBR-style rate control
/// (see [`VideoEncoderConfig::rate_control`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateControlConfig {
    /// Target bitrate in bits per second.
    pub target_bitrate_bps: u32,
    /// VBV buffer size in bytes. `None` lets the backend pick a
    /// driver-suggested default rather than this crate guessing one.
    pub vbv_buffer_size_bytes: Option<u32>,
}

impl VideoEncoderConfig {
    /// H.264 defaults for a given size (tests / demos). Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn h264(width: u32, height: u32, time_base: Rational) -> Self {
        Self {
            codec: CodecKind::H264,
            width,
            height,
            time_base,
            bitrate_bps: 0,
            pixel_format: PixelFormat::Nv12,
            color_range: ColorRange::Video,
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: None,
            gop_size: 1,
            rate_control: None,
            intra_refresh_period: None,
        }
    }

    /// HEVC defaults for a given size. Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn hevc(width: u32, height: u32, time_base: Rational) -> Self {
        Self {
            codec: CodecKind::Hevc,
            width,
            height,
            time_base,
            bitrate_bps: 0,
            pixel_format: PixelFormat::Nv12,
            color_range: ColorRange::Video,
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: None,
            gop_size: 1,
            rate_control: None,
            intra_refresh_period: None,
        }
    }

    /// AV1 defaults for a given size. Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn av1(width: u32, height: u32, time_base: Rational) -> Self {
        Self {
            codec: CodecKind::Av1,
            width,
            height,
            time_base,
            bitrate_bps: 0,
            pixel_format: PixelFormat::Nv12,
            color_range: ColorRange::Video,
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: None,
            gop_size: 1,
            rate_control: None,
            intra_refresh_period: None,
        }
    }

    /// VP9 defaults for a given size. Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn vp9(width: u32, height: u32, time_base: Rational) -> Self {
        Self {
            codec: CodecKind::Vp9,
            width,
            height,
            time_base,
            bitrate_bps: 0,
            pixel_format: PixelFormat::Nv12,
            color_range: ColorRange::Video,
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: None,
            gop_size: 1,
            rate_control: None,
            intra_refresh_period: None,
        }
    }
}

/// Streaming hardware (or backend) video encoder.
///
/// Push frames, then [`poll_packet`](VideoEncoder::poll_packet) until `Ok(None)`,
/// then [`flush`](VideoEncoder::flush) and drain again.
pub trait VideoEncoder {
    /// Stream metadata (updated when extradata becomes available).
    fn stream_info(&self) -> &StreamInfo;

    /// Submit one frame. May produce zero or more packets (drain via poll).
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] when the frame is rejected or the session failed.
    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError>;

    /// Pull the next compressed packet, if any.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] on backend failure.
    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError>;

    /// Signal end-of-input; drain remaining packets with [`poll_packet`](Self::poll_packet).
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] on backend failure.
    fn flush(&mut self) -> Result<(), EncodeError>;

    /// Retarget the live CBR bitrate ceiling, taking effect from the next
    /// [`push_frame`](Self::push_frame) call — no session reopen, no dropped frames.
    ///
    /// Only meaningful for a session opened with
    /// [`VideoEncoderConfig::rate_control`] set (CBR-style rate control); a
    /// fixed-QP session has no bitrate ceiling to retarget. The default
    /// implementation always returns [`EncodeError::Unsupported`] — backends
    /// that support live CBR retargeting override this method and must
    /// document the honored range/granularity on their own encoder type's
    /// rustdoc, per `caveats-and-clarity.md`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] when the session is not in CBR
    /// mode or this backend cannot retarget bitrate live; [`EncodeError`] on
    /// backend failure otherwise.
    fn set_bitrate(&mut self, _bitrate_bps: u32) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

/// Forwarding impl so `Box<dyn VideoEncoder>` (cross-platform dispatch) satisfies
/// [`VideoEncoder`] directly — mirrors `impl<R: Read + ?Sized> Read for Box<R>` in
/// `std::io`. Callers holding a concrete encoder type pay no `Box` at all; callers
/// holding `Box<dyn VideoEncoder>` don't need an extra wrapper to use it generically.
impl<T: VideoEncoder + ?Sized> VideoEncoder for Box<T> {
    fn stream_info(&self) -> &StreamInfo {
        (**self).stream_info()
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        (**self).push_frame(frame)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        (**self).poll_packet()
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        (**self).flush()
    }

    fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<(), EncodeError> {
        (**self).set_bitrate(bitrate_bps)
    }
}
