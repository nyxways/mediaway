//! Linux decode backend (VA-API, via `cros-libva`).
//!
//! - [`VideoOutputPreference::CpuFramesOk`](crate::VideoOutputPreference): H.264
//!   decode through VA-API's `VAEntrypointVLD` pipeline (`vaCreateConfig`/`vaCreateContext`,
//!   per-picture `vaBeginPicture`/`vaRenderPicture`/`vaEndPicture`/`vaSyncSurface`), then NV12
//!   read back via `vaCreateImage`/`vaGetImage`/`vaMapBuffer` into an owned `Bytes` — no
//!   DMA-BUF export, so this is honest CPU decode, not a disguised Zero-Copy path.
//! - [`VideoOutputPreference::ZeroCopyGpu`]: not implemented — returns
//!   [`DecodeError::Unsupported`]. Deferred to a Zero-Copy DMA-BUF stage; see
//!   [`docs/roadmap.md`](../docs/roadmap.md).
//!
//! Policy: [ADR-0001](../adr/0001-vaapi-h264-cpu-out.md) — binding choice (`cros-libva`),
//! decode scope (IDR pictures only, single slice per picture, `pic_order_cnt_type == 0`,
//! baseline/main profile), and the **zero real-hardware verification** caveat for this crate
//! as authored (compile-verified on Linux only; no VA-API driver was available to run
//! against). Read that caveat before relying on this backend.

// Unlike `mediaway-decoder-windows`, this crate writes no raw FFI `unsafe` of its own: all
// VA-API calls go through `cros-libva`'s safe wrapper layer (`Display`/`Config`/`Context`/
// `Surface`/`Picture`/`Buffer`). See ADR-0001 for why that safe surface is sufficient for this
// crate's scope — mirrors `mediaway-encoder-linux`'s identical choice.
#![forbid(unsafe_code)]

#[cfg(not(feature = "video"))]
compile_error!("enable the `video` feature on mediaway-decoder-linux");

use crate::DecodeError;
#[cfg(feature = "video")]
use crate::{VideoDecoder, VideoDecoderConfig};
#[cfg(feature = "video")]
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

#[cfg(target_os = "linux")]
mod vaapi;

/// Linux video decode session (VA-API H.264/AV1/VP9 when opened on Linux).
#[cfg(feature = "video")]
pub struct LinuxVideoDecoder {
    #[cfg(target_os = "linux")]
    inner: Option<vaapi::VaapiVideoDecoder>,
    #[cfg(not(target_os = "linux"))]
    _priv: (),
}

#[cfg(feature = "video")]
impl LinuxVideoDecoder {
    /// Open a Linux VA-API video decoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Unsupported`] when the codec/output path is not wired
    /// (currently: anything but H.264/AV1 `KEY_FRAME`-only/VP9 +
    /// [`VideoOutputPreference::CpuFramesOk`]), or [`DecodeError::Backend`] on VA-API failure.
    /// No `/dev/dri/renderD*` VA-API display is expected in most CI/dev environments — see
    /// ADR-0001's hardware caveat.
    ///
    /// [`VideoOutputPreference::CpuFramesOk`]: crate::VideoOutputPreference::CpuFramesOk
    #[cfg(target_os = "linux")]
    pub fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        let inner = vaapi::VaapiVideoDecoder::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Linux build: decoder unavailable.
    #[cfg(not(target_os = "linux"))]
    pub const fn open(_config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        Err(DecodeError::Unsupported)
    }
}

#[cfg(feature = "video")]
#[cfg(target_os = "linux")]
impl VideoDecoder for LinuxVideoDecoder {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(d) = self.inner.as_ref() {
            d.stream_info()
        } else {
            closed_stream_info()
        }
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        self.inner
            .as_mut()
            .ok_or(DecodeError::Closed)?
            .push_packet(packet)
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        self.inner.as_mut().ok_or(DecodeError::Closed)?.poll_frame()
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        self.inner.as_mut().ok_or(DecodeError::Closed)?.flush()
    }
}

#[cfg(feature = "video")]
#[cfg(not(target_os = "linux"))]
impl VideoDecoder for LinuxVideoDecoder {
    fn stream_info(&self) -> &StreamInfo {
        closed_stream_info()
    }

    fn push_packet(&mut self, _packet: &Packet) -> Result<(), DecodeError> {
        Err(DecodeError::Unsupported)
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        Ok(None)
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        Err(DecodeError::Unsupported)
    }
}

#[cfg(feature = "video")]
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

#[cfg(all(test, target_os = "linux", feature = "video"))]
#[path = "lib_tests.rs"]
mod tests;
