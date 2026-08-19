//! Linux encode backend (VA-API, via `cros-libva`).
//!
//! - [`VideoInputPreference::CpuUploadOk`](crate::VideoInputPreference): H.264
//!   Constrained Baseline `VAEntrypointEncSlice` session (CQP rate control) +
//!   `upload_cpu_nv12` (copy) — every pushed frame is encoded as an independent IDR
//!   intra frame (no GOP / P-frame reference management this stage).
//! - [`VideoInputPreference::ZeroCopyGpu`]: DMA-BUF import via `vaCreateSurfaces`
//!   (`vaapi::dmabuf`) — no CPU upload; a caller-supplied
//!   [`mediaway_common::GpuBufferHandle::DmaBuf`] surface is imported and encoded directly.
//!   Forces all-IDR encode for the session (no GOP / P-frame references) — each imported
//!   surface is single-use, never held across calls; see
//!   `adr/linux/0003-vaapi-dmabuf-zero-copy-input.md`.
//!
//! Policy: [ADR-0001](../adr/0001-vaapi-cros-libva-h264-cpu-upload.md) — binding choice
//! (`cros-libva`), scope, and the **zero real-hardware verification** caveat for this crate
//! as authored (compile-verified on Linux only; no VA-API driver was available to run
//! against). Read that caveat before relying on this backend.

// Most VA-API calls go through `cros-libva`'s safe wrapper layer (`Display`/`Config`/`Context`/
// `Surface`/`Picture`/`Buffer`) — see ADR-0001. `#[allow]`, not `#[forbid]`, because
// `vaapi::dmabuf` (ADR-0003) must reconstruct a `BorrowedFd` from a caller-supplied raw fd
// number, which `std` itself only exposes as an `unsafe fn` — see that module's doc comment.
#![allow(unsafe_code)]

#[cfg(not(feature = "video"))]
compile_error!("enable the `video` feature on mediaway-encoder-linux");

use crate::EncodeError;
use crate::{VideoEncoder, VideoEncoderConfig};
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

#[cfg(target_os = "linux")]
mod vaapi;

/// Linux video encode session (VA-API H.264 when opened on Linux).
pub struct LinuxVideoEncoder {
    #[cfg(target_os = "linux")]
    inner: Option<vaapi::VaapiVideoEncoder>,
    #[cfg(not(target_os = "linux"))]
    _priv: (),
}

impl LinuxVideoEncoder {
    /// Open a Linux VA-API video encoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] when the codec is not wired (currently: anything
    /// but H.264), or [`EncodeError::Backend`] when no VA-API display/driver is available
    /// (expected in any environment without a real `/dev/dri/renderD*` VA-API device — see
    /// ADR-0001).
    #[cfg(target_os = "linux")]
    pub fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let inner = vaapi::VaapiVideoEncoder::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Linux build: encoder unavailable.
    #[cfg(not(target_os = "linux"))]
    pub const fn open(_config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(target_os = "linux")]
impl VideoEncoder for LinuxVideoEncoder {
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
}

#[cfg(not(target_os = "linux"))]
impl VideoEncoder for LinuxVideoEncoder {
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

#[cfg(all(test, target_os = "linux"))]
#[path = "lib_tests.rs"]
mod tests;
