//! Intel Quick Sync / Arc (oneVPL) video encode backend.
//!
//! - [`VideoInputPreference::CpuUploadOk`](mediaway_encoder::VideoInputPreference::CpuUploadOk):
//!   H.264 Baseline profile, real I/P GOP structure (driver-managed
//!   reference lists — not all-IDR), CPU NV12 `upload_cpu_nv12` (copy).
//! - [`VideoInputPreference::ZeroCopyGpu`](mediaway_encoder::VideoInputPreference::ZeroCopyGpu):
//!   not implemented — returns [`EncodeError::Unsupported`]. Deferred to the
//!   D3D11 external-allocator Zero-Copy stage; see
//!   [`docs/roadmap.md`](../docs/roadmap.md).
//!
//! Windows only this stage (real oneVPL runtime + hardware verification both
//! only available on this workspace's Windows dev box) — this crate must stay
//! buildable (as an honest `Unsupported` stub) on non-Windows hosts running
//! `cargo check --workspace`. Mirrors `mediaway-encoder-linux`'s
//! `cfg(not(target_os = "linux"))` stub pattern.
//!
//! Policy: [ADR-0001](../adr/0001-onevpl-quicksync-encode-surface.md) — binding
//! choice (`vpl-sys`, hand-rolled `libloading` MVP dispatcher), scope, and the
//! 2026-07-29 addendum documenting what was actually built/hardware-verified.

// Not `forbid`: `quicksync` (Windows real backend) needs one localized
// `#[allow(unsafe_code)]` for a raw-slice read of the oneVPL output
// bitstream — `forbid` cannot be overridden downstream (see AGENTS.md
// Absolute rules § Safety & code hygiene). This module itself writes no
// `unsafe` and stays on the workspace-default `unsafe_code = "deny"`.

#[cfg(not(feature = "video"))]
compile_error!("enable the `video` feature on mediaway-encoder-quicksync");

use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};
use mediaway_encoder::EncodeError;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig};

#[cfg(windows)]
mod quicksync;

/// Quick Sync / Arc video encode session (oneVPL H.264 CPU-upload when opened on Windows).
pub struct QuickSyncVideoEncoder {
    #[cfg(windows)]
    inner: Option<quicksync::QuickSyncSession>,
    #[cfg(not(windows))]
    _priv: (),
}

impl QuickSyncVideoEncoder {
    /// Open a Quick Sync / Arc (oneVPL) video encoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] when the codec/input path is not
    /// wired (currently: anything but H.264 +
    /// [`VideoInputPreference::CpuUploadOk`](mediaway_encoder::VideoInputPreference::CpuUploadOk)),
    /// or [`EncodeError::Backend`] when no oneVPL runtime / Intel GPU
    /// implementation is available (expected on any host without the Intel
    /// graphics driver package installed — see this crate's `adr/0001`).
    #[cfg(windows)]
    pub fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let inner = quicksync::QuickSyncSession::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Windows build: encoder unavailable.
    #[cfg(not(windows))]
    pub const fn open(_config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(windows)]
impl VideoEncoder for QuickSyncVideoEncoder {
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
impl VideoEncoder for QuickSyncVideoEncoder {
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
