//! VA-API encode session (Linux only), built on the safe `cros-libva` wrapper.
//!
//! See [ADR-0001](../../adr/0001-vaapi-cros-libva-h264-cpu-upload.md) for the binding
//! choice, scope, and the zero-hardware-verification caveat. [ADR-0003](../../adr/linux/0003-vaapi-hevc-p-frame-gop.md)
//! adds HEVC encode alongside H.264, unified behind [`VaapiVideoSession`] (an enum, not
//! `Box<dyn VideoEncoder>` — see that ADR's § ZCA shape).

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

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig};
use mediaway_common::{CodecKind, Packet, StreamInfo, VideoFrame};

/// Dispatches encode calls to whichever codec-specific VA-API session `open` resolved —
/// [`video::VaapiVideoEncoder`] (H.264) or [`hevc::VaapiHevcVideoEncoder`] (HEVC, ADR-0003).
/// Two closed, concrete variants — no `dyn Trait`/`Box<dyn _>` anywhere in this dispatch,
/// matching every other encode/decode backend in this workspace (ADR-0003 § ZCA shape).
pub(crate) enum VaapiVideoSession {
    H264(video::VaapiVideoEncoder),
    Hevc(hevc::VaapiHevcVideoEncoder),
}

impl VaapiVideoSession {
    /// Opens the codec-specific VA-API session for `config.codec`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] for any codec but H.264/HEVC, or whatever the
    /// chosen codec's own `open` returns otherwise.
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        match config.codec {
            CodecKind::H264 => Ok(Self::H264(video::VaapiVideoEncoder::open(config)?)),
            CodecKind::Hevc => Ok(Self::Hevc(hevc::VaapiHevcVideoEncoder::open(config)?)),
            _ => Err(EncodeError::Unsupported),
        }
    }
}

impl VideoEncoder for VaapiVideoSession {
    fn stream_info(&self) -> &StreamInfo {
        match self {
            Self::H264(e) => e.stream_info(),
            Self::Hevc(e) => e.stream_info(),
        }
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        match self {
            Self::H264(e) => e.push_frame(frame),
            Self::Hevc(e) => e.push_frame(frame),
        }
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        match self {
            Self::H264(e) => e.poll_packet(),
            Self::Hevc(e) => e.poll_packet(),
        }
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        match self {
            Self::H264(e) => e.flush(),
            Self::Hevc(e) => e.flush(),
        }
    }
}
