//! AMD AMF encode backend (`shiguredo_amf`, Linux `x86_64` only).
//!
//! - [`VideoInputPreference::CpuUploadOk`](crate::VideoInputPreference): H.264 / HEVC / AV1
//!   CPU NV12 upload (`Surface::get_plane` raw-pointer write) — a genuine
//!   CPU→driver copy, matching every other Stage-1 backend's `upload_cpu_nv12`
//!   cost-disclosure convention. VP9 is unsupported — `shiguredo_amf` has no `CodecConfig`
//!   variant for it (see `adr/amf/0003`).
//! - [`VideoInputPreference::ZeroCopyGpu`](crate::VideoInputPreference::ZeroCopyGpu):
//!   not implemented — returns [`EncodeError::Unsupported`]. No GPU-surface-import
//!   type is confirmed to exist in `shiguredo_amf` at all (see this crate's
//!   `adr/amf/0002`).
//!
//! `shiguredo_amf` is `x86_64-unknown-linux-gnu` only (its own platform limit, not a
//! Mediaway restriction) — this crate must stay buildable (as an honest `Unsupported`
//! stub) on every other host running `cargo check --workspace`. Mirrors
//! `mediaway-encoder::linux`'s `cfg(not(target_os = "linux"))` stub pattern, with the
//! additional `target_arch = "x86_64"` gate `linux`'s VA-API backend does not need.
//!
//! Policy: [ADR-0001](../adr/amf/0001-amf-deferred-no-hardware.md) (research, deferred),
//! [ADR-0002](../adr/amf/0002-amf-linux-shiguredo-amf-h264-cpu-upload.md) (accepted design +
//! H.264 implementation), [ADR-0003](../adr/amf/0003-amf-linux-hevc-av1-codec-dispatch.md)
//! (HEVC/AV1 codec dispatch) — binding choice (`shiguredo_amf`, never `amf-rs`), scope, the
//! callback→poll bridge design, and the **zero real-hardware verification** caveat for this
//! backend as authored (compile-verified on Linux `x86_64` only via WSL2 — no AMD GPU/driver
//! exists anywhere in this workspace's sessions). Read that caveat before relying on this
//! backend.

#[cfg(not(feature = "video"))]
compile_error!("enable the `video` feature on mediaway-encoder");

use crate::EncodeError;
use crate::{VideoEncoder, VideoEncoderConfig};
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux;

/// AMD AMF video encode session (`shiguredo_amf` H.264/HEVC/AV1 CPU-upload when opened on
/// Linux `x86_64`).
pub struct AmfVideoEncoder {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    inner: Option<linux::AmfSession>,
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    _priv: (),
}

impl AmfVideoEncoder {
    /// Open an AMD AMF video encoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] when the codec/input path is not wired
    /// (currently: anything but H.264/HEVC/AV1 + [`VideoInputPreference::CpuUploadOk`] — VP9
    /// stays unsupported, see `adr/amf/0003`), or [`EncodeError::Backend`] when no AMD AMF
    /// runtime/driver is available (expected in any environment without a real AMD GPU +
    /// driver — see `adr/amf/0002`).
    ///
    /// [`VideoInputPreference::CpuUploadOk`]: crate::VideoInputPreference::CpuUploadOk
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let inner = linux::AmfSession::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host build outside Linux `x86_64`: encoder unavailable (`shiguredo_amf`'s own
    /// platform limit — see module docs).
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    pub const fn open(_config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl VideoEncoder for AmfVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(e) = self.inner.as_ref() {
            e.stream_info()
        } else {
            closed_stream_info()
        }
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        self.inner
            .as_mut()
            .ok_or(EncodeError::Closed)?
            .push_frame(frame)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        self.inner
            .as_mut()
            .ok_or(EncodeError::Closed)?
            .poll_packet()
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        self.inner.as_mut().ok_or(EncodeError::Closed)?.flush()
    }

    fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<(), EncodeError> {
        self.inner
            .as_mut()
            .ok_or(EncodeError::Closed)?
            .set_bitrate(bitrate_bps)
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
impl VideoEncoder for AmfVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        closed_stream_info()
    }

    fn push_frame(&mut self, _frame: &VideoFrame) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(None)
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

fn closed_stream_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Video {
        id: 0,
        codec: mediaway_common::CodecKind::H264,
        time_base: mediaway_common::Rational::new(1, 30),
        geometry: mediaway_common::VideoGeometry {
            width: 0,
            height: 0,
        },
        extra_data: Bytes::new(),
    })
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "lib_tests.rs"]
mod tests;
