//! VA-API encode session (Linux only), built on the safe `cros-libva` wrapper.
//!
//! See [ADR-0001](../../adr/0001-vaapi-cros-libva-h264-cpu-upload.md) for the binding
//! choice, scope, and the zero-hardware-verification caveat. [ADR-0003](../../adr/linux/0003-vaapi-hevc-p-frame-gop.md)
//! adds HEVC encode and [ADR-0004](../../adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md)
//! adds VP9 encode alongside H.264, all unified behind [`VaapiVideoEncoder`] (an enum, not
//! `Box<dyn VideoEncoder>` — see `docs/spec/zero-cost-abstractions.md`). AV1 encode is
//! designed but blocked — see [ADR-0003](../../adr/linux/0003-vaapi-av1-key-frame-and-inter-gop.md)
//! § Why VP9 does not share AV1's packed-header blocker.

// No raw FFI `unsafe` in this crate — see `crate` root doc comment / ADR-0001.
#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for VA-API modules; not part of the public crate API"
)]

mod codec;
mod gop;
mod hevc;
mod hevc_gop;
mod video;
mod vp9;
mod vp9_gop;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig};
use mediaway_common::{CodecKind, Packet, StreamInfo, VideoFrame};

use hevc::VaapiHevcVideoEncoder;
use video::VaapiH264Encoder;
use vp9::VaapiVp9Encoder;

/// Dispatches to the right per-codec VA-API encode session based on
/// [`VideoEncoderConfig::codec`] — a plain enum over this crate's concrete encoder types rather
/// than `Box<dyn VideoEncoder>` (see `docs/spec/zero-cost-abstractions.md`).
pub(crate) enum VaapiVideoEncoder {
    H264(VaapiH264Encoder),
    Hevc(VaapiHevcVideoEncoder),
    Vp9(VaapiVp9Encoder),
}

impl VaapiVideoEncoder {
    /// Open the per-codec VA-API encoder matching `config.codec`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] for any codec this vaapi backend does not encode at
    /// all, or the chosen encoder's own `open` errors otherwise.
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        if !codec::is_supported_video_codec(config.codec) {
            return Err(EncodeError::Unsupported);
        }
        match config.codec {
            CodecKind::H264 => Ok(Self::H264(VaapiH264Encoder::open(config)?)),
            CodecKind::Hevc => Ok(Self::Hevc(VaapiHevcVideoEncoder::open(config)?)),
            CodecKind::Vp9 => Ok(Self::Vp9(VaapiVp9Encoder::open(config)?)),
            _ => Err(EncodeError::Unsupported),
        }
    }
}

impl VideoEncoder for VaapiVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        match self {
            Self::H264(e) => e.stream_info(),
            Self::Hevc(e) => e.stream_info(),
            Self::Vp9(e) => e.stream_info(),
        }
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        match self {
            Self::H264(e) => e.push_frame(frame),
            Self::Hevc(e) => e.push_frame(frame),
            Self::Vp9(e) => e.push_frame(frame),
        }
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        match self {
            Self::H264(e) => e.poll_packet(),
            Self::Hevc(e) => e.poll_packet(),
            Self::Vp9(e) => e.poll_packet(),
        }
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        match self {
            Self::H264(e) => e.flush(),
            Self::Hevc(e) => e.flush(),
            Self::Vp9(e) => e.flush(),
        }
    }
}
