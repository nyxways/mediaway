//! VA-API decode helpers (Linux, `cros-libva`).
//!
//! [ADR-0003](../../adr/linux/0003-vaapi-hevc-p-slice-dpb.md) adds HEVC decode alongside H.264,
//! unified behind [`VaapiVideoSession`] (an enum, not `Box<dyn VideoDecoder>` — mirrors the
//! sibling encoder's identical `VaapiVideoSession` shape, `mediaway-encoder` ADR-0003).

#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for VA-API modules; not part of the public crate API — mirrors mediaway-decoder-windows's wmf/mod.rs"
)]

mod codec;
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

use crate::{DecodeError, VideoDecoder, VideoDecoderConfig};
use mediaway_common::{CodecKind, Packet, StreamInfo, VideoFrame};

/// Dispatches decode calls to whichever codec-specific VA-API session `open` resolved —
/// [`h264::VaapiH264Decoder`] or [`hevc::VaapiHevcDecoder`] (ADR-0003). Two closed, concrete
/// variants — no `dyn Trait`/`Box<dyn _>` anywhere in this dispatch, matching every other
/// encode/decode backend in this workspace.
pub(crate) enum VaapiVideoSession {
    H264(h264::VaapiH264Decoder),
    Hevc(hevc::VaapiHevcDecoder),
}

impl VaapiVideoSession {
    /// Opens the codec-specific VA-API session for `config.codec`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Unsupported`] for any codec but H.264/HEVC, or whatever the
    /// chosen codec's own `open` returns otherwise.
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        match config.codec {
            CodecKind::H264 => Ok(Self::H264(h264::VaapiH264Decoder::open(config)?)),
            CodecKind::Hevc => Ok(Self::Hevc(hevc::VaapiHevcDecoder::open(config)?)),
            _ => Err(DecodeError::Unsupported),
        }
    }
}

impl VideoDecoder for VaapiVideoSession {
    fn stream_info(&self) -> &StreamInfo {
        match self {
            Self::H264(d) => d.stream_info(),
            Self::Hevc(d) => d.stream_info(),
        }
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        match self {
            Self::H264(d) => d.push_packet(packet),
            Self::Hevc(d) => d.push_packet(packet),
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        match self {
            Self::H264(d) => d.poll_frame(),
            Self::Hevc(d) => d.poll_frame(),
        }
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        match self {
            Self::H264(d) => d.flush(),
            Self::Hevc(d) => d.flush(),
        }
    }
}
