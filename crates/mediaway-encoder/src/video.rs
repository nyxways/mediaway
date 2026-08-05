//! Video encode config and [`VideoEncoder`] trait.

#![forbid(unsafe_code)]

use crate::error::EncodeError;
use mediaway_common::{
    CodecKind, GpuDeviceHandle, Packet, PixelFormat, Rational, StreamInfo, VideoFrame,
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
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: None,
            gop_size: 1,
            rate_control: None,
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
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: None,
            gop_size: 1,
            rate_control: None,
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
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: None,
            gop_size: 1,
            rate_control: None,
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
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: None,
            gop_size: 1,
            rate_control: None,
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
}
