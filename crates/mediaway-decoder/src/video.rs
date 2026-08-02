//! Video decode config and [`VideoDecoder`] trait.

#![forbid(unsafe_code)]

use crate::error::DecodeError;
use mediaway_common::{
    CodecKind, GpuDeviceHandle, Packet, PixelFormat, Rational, StreamInfo, VideoFrame,
};

/// How the caller prefers to receive decoded frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum VideoOutputPreference {
    /// Prefer GPU handles ([`mediaway_common::VideoFrameStorage::Gpu`]).
    #[default]
    ZeroCopyGpu,
    /// Accept CPU frames (may imply copy/readback — backends must document cost).
    CpuFramesOk,
}

/// Parameters for opening a video decoder session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoDecoderConfig {
    /// Input codec (Stage 1 Windows: [`CodecKind::H264`]).
    pub codec: CodecKind,
    /// Expected width (may be refined from bitstream).
    pub width: u32,
    /// Expected height (may be refined from bitstream).
    pub height: u32,
    /// Timestamp timebase for input packets and output frames.
    pub time_base: Rational,
    /// Preferred output pixel format when the backend converts.
    pub pixel_format: PixelFormat,
    /// Output path preference (Zero-Copy vs CPU).
    pub output: VideoOutputPreference,
    /// GPU device handle when [`VideoOutputPreference::ZeroCopyGpu`].
    ///
    /// `None` means unset (Zero-Copy open fails). `Some(GpuDeviceHandle::DirectX11(handle))`
    /// specifies the device that owns returned textures; other variants select other backends
    /// (see [`GpuDeviceHandle`](mediaway_common::GpuDeviceHandle) for platform options).
    pub gpu_device: Option<GpuDeviceHandle>,
    /// Codec configuration bytes (AVCC / extradata); may be empty until first keyframe.
    pub extra_data: mediaway_common::Bytes,
}

impl VideoDecoderConfig {
    /// H.264 defaults for a given size. Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn h264(width: u32, height: u32, time_base: Rational) -> Self {
        Self {
            codec: CodecKind::H264,
            width,
            height,
            time_base,
            pixel_format: PixelFormat::Nv12,
            output: VideoOutputPreference::ZeroCopyGpu,
            gpu_device: None,
            extra_data: mediaway_common::Bytes::new(),
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
            pixel_format: PixelFormat::Nv12,
            output: VideoOutputPreference::ZeroCopyGpu,
            gpu_device: None,
            extra_data: mediaway_common::Bytes::new(),
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
            pixel_format: PixelFormat::Nv12,
            output: VideoOutputPreference::ZeroCopyGpu,
            gpu_device: None,
            extra_data: mediaway_common::Bytes::new(),
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
            pixel_format: PixelFormat::Nv12,
            output: VideoOutputPreference::ZeroCopyGpu,
            gpu_device: None,
            extra_data: mediaway_common::Bytes::new(),
        }
    }
}

/// Streaming hardware (or backend) video decoder.
///
/// Push packets, then [`poll_frame`](VideoDecoder::poll_frame) until `Ok(None)`,
/// then [`flush`](VideoDecoder::flush) and drain again.
pub trait VideoDecoder {
    /// Stream metadata (updated when size / extradata become available).
    fn stream_info(&self) -> &StreamInfo;

    /// Submit one compressed packet. May produce zero or more frames (drain via poll).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when the packet is rejected or the session failed.
    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError>;

    /// Pull the next decoded frame, if any.
    ///
    /// For GPU frames, the texture remains valid until the next
    /// [`push_packet`](Self::push_packet) / [`poll_frame`](Self::poll_frame) /
    /// [`flush`](Self::flush) that recycles the surface (see platform ADR).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] on backend failure.
    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError>;

    /// Signal end-of-input; drain remaining frames with [`poll_frame`](Self::poll_frame).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] on backend failure.
    fn flush(&mut self) -> Result<(), DecodeError>;
}
