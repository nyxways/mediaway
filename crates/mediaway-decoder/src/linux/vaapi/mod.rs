//! VA-API decode helpers (Linux, `cros-libva`).
//!
//! [ADR-0003](../../adr/linux/0003-vaapi-hevc-p-slice-dpb.md) adds HEVC decode,
//! [ADR-0005](../../adr/linux/0005-vaapi-av1-key-frame-decode.md) adds AV1 decode, and
//! [ADR-0004](../../adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md) adds VP9 decode
//! alongside H.264, all unified behind [`VaapiVideoDecoder`] (an enum, not `Box<dyn
//! VideoDecoder>` — see `docs/spec/zero-cost-abstractions.md`).

#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for VA-API modules; not part of the public crate API — mirrors mediaway-decoder-windows's wmf/mod.rs"
)]

mod av1;
mod codec;
mod dmabuf;
mod dpb;
mod h264;
mod hevc;
mod hevc_dpb;
mod hevc_nal;
mod hevc_pps;
mod hevc_slice;
mod hevc_sps;
mod nv12;
mod pps;
mod slice;
mod sps;
mod vp9;

use crate::{DecodeError, VideoDecoder, VideoDecoderConfig};
use mediaway_common::{CodecKind, Packet, StreamInfo, VideoFrame};

use av1::VaapiAv1Decoder;
use h264::VaapiH264Decoder;
use hevc::VaapiHevcDecoder;
use vp9::VaapiVp9Decoder;

/// Dispatches to the right per-codec VA-API decode session based on
/// [`VideoDecoderConfig::codec`] — a plain enum over this crate's concrete decoder types
/// rather than `Box<dyn VideoDecoder>` (see `docs/spec/zero-cost-abstractions.md`).
pub(crate) enum VaapiVideoDecoder {
    H264(VaapiH264Decoder),
    Hevc(VaapiHevcDecoder),
    Av1(VaapiAv1Decoder),
    Vp9(VaapiVp9Decoder),
}

impl VaapiVideoDecoder {
    /// Open the per-codec VA-API decoder matching `config.codec`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Unsupported`] for any codec this vaapi backend does not decode at
    /// all, or the chosen decoder's own `open` errors otherwise.
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        if !codec::is_supported_video_codec(config.codec) {
            return Err(DecodeError::Unsupported);
        }
        match config.codec {
            CodecKind::H264 => Ok(Self::H264(VaapiH264Decoder::open(config)?)),
            CodecKind::Hevc => Ok(Self::Hevc(VaapiHevcDecoder::open(config)?)),
            CodecKind::Av1 => Ok(Self::Av1(VaapiAv1Decoder::open(config)?)),
            CodecKind::Vp9 => Ok(Self::Vp9(VaapiVp9Decoder::open(config)?)),
            _ => Err(DecodeError::Unsupported),
        }
    }
}

impl VideoDecoder for VaapiVideoDecoder {
    fn stream_info(&self) -> &StreamInfo {
        match self {
            Self::H264(d) => d.stream_info(),
            Self::Hevc(d) => d.stream_info(),
            Self::Av1(d) => d.stream_info(),
            Self::Vp9(d) => d.stream_info(),
        }
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        match self {
            Self::H264(d) => d.push_packet(packet),
            Self::Hevc(d) => d.push_packet(packet),
            Self::Av1(d) => d.push_packet(packet),
            Self::Vp9(d) => d.push_packet(packet),
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        match self {
            Self::H264(d) => d.poll_frame(),
            Self::Hevc(d) => d.poll_frame(),
            Self::Av1(d) => d.poll_frame(),
            Self::Vp9(d) => d.poll_frame(),
        }
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        match self {
            Self::H264(d) => d.flush(),
            Self::Hevc(d) => d.flush(),
            Self::Av1(d) => d.flush(),
            Self::Vp9(d) => d.flush(),
        }
    }
}
