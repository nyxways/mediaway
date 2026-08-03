//! NVIDIA NVENC direct vendor video encode backend (`Backend::Nvenc` — see
//! `mediaway-encoder` ADR-0004).
//!
//! - [`VideoInputPreference::CpuUploadOk`](crate::VideoInputPreference): H.264 /
//!   HEVC / AV1 session (`NV_ENC_DEVICE_TYPE_DIRECTX`) opened on a private, internally-owned
//!   D3D11 device. Pushed CPU NV12 frames are uploaded into a private D3D11 staging texture
//!   (`upload_cpu_nv12` — copy, see [`dx11::device`]) and `CopyResource`'d into a
//!   GPU-resident texture registered once with NVENC — never Zero-Copy this stage.
//! - [`VideoInputPreference::ZeroCopyGpu`]: not implemented — returns
//!   [`EncodeError::Unsupported`]. Deferred to a caller-supplied-texture Zero-Copy stage;
//!   see [`docs/roadmap.md`](../docs/roadmap.md).
//!
//! Policy: [ADR-0001](../adr/0001-nvenc-vendor-backend.md) — bindings choice (the `nvenc`
//! crate, depended on directly, not vendored), a real hardware-verified bug in that crate's
//! native CPU input-buffer lock path (worked around via the D3D11 texture path above), and
//! this stage's scope. Read the 2026-07-29 addenda (H.264 CPU-upload, then HEVC/AV1) before
//! relying on this backend.
//!
//! Windows-only this stage (`#[cfg(windows)]`, mirroring `mediaway-encoder-windows` /
//! `mediaway-encoder-linux`'s own cross-compile stub pattern) — other targets get an honest
//! [`EncodeError::Unsupported`] stub.

// D3D11 device/texture calls and the `nvenc` crate's FFI surface are unavoidable for a
// vendor GPU encode backend — every `unsafe` block in `dx11::device` carries a `// SAFETY:`
// comment. Matches `mediaway-encoder-windows`'s `wmf` modules (code-style.md).
#![allow(unsafe_code)]

#[cfg(not(feature = "video"))]
compile_error!("enable the `video` feature on mediaway-encoder-nvenc");

use crate::EncodeError;
use crate::{VideoEncoder, VideoEncoderConfig};
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

#[cfg(windows)]
mod dx11;

/// NVENC video encode session (H.264 / HEVC / AV1 CPU-upload when opened on Windows).
pub struct NvencVideoEncoder {
    #[cfg(windows)]
    inner: Option<dx11::NvencSession>,
    #[cfg(not(windows))]
    _priv: (),
}

impl NvencVideoEncoder {
    /// Open an NVENC H.264 / HEVC / AV1 CPU-upload video encoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] when the codec/input path is not wired
    /// (currently: anything but H.264/HEVC/AV1 + [`VideoInputPreference::CpuUploadOk`](
    /// crate::VideoInputPreference::CpuUploadOk); NVENC has no VP9 encoder at
    /// all, see [ADR-0001](../adr/0001-nvenc-vendor-backend.md)), or [`EncodeError::Backend`]
    /// when no usable NVIDIA GPU/driver/NVENC session is available (missing
    /// `nvEncodeAPI64.dll`, no NVENC-capable device, or a D3D11/NVENC call failed).
    #[cfg(windows)]
    pub fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let inner = dx11::NvencSession::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Non-Windows build: NVENC backend unavailable this stage.
    #[cfg(not(windows))]
    pub const fn open(_config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(windows)]
impl VideoEncoder for NvencVideoEncoder {
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

#[cfg(not(windows))]
impl VideoEncoder for NvencVideoEncoder {
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

#[cfg(all(test, windows))]
#[path = "lib_tests.rs"]
mod tests;
