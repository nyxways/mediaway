//! Android decode backend (NDK `AMediaCodec`, via the `ndk` crate).
//!
//! - [`VideoOutputPreference::CpuFramesOk`](crate::VideoOutputPreference): H.264 (`video/avc`)
//!   decode via `ndk::media::media_codec::MediaCodec` (`configure`+`start` eagerly at
//!   `open()`, no `Surface`). Output color format is restricted to
//!   `COLOR_FormatYUV420SemiPlanar` (`21`) — any other reported format is an honest
//!   [`DecodeError::Unsupported`], never a silent misinterpretation; `stride`/`slice-height`/
//!   crop are read from the negotiated output format and stripped into a tightly packed NV12
//!   `Bytes` — a genuine driver→CPU copy, not a disguised Zero-Copy path. General H.264 GOP
//!   (P/B frames), not IDR-only — `AMediaCodec` manages its own DPB internally.
//! - [`VideoOutputPreference::ZeroCopyGpu`]: not implemented — returns
//!   [`DecodeError::Unsupported`]. Deferred to a Zero-Copy `AHardwareBuffer`/`ANativeWindow`/
//!   `Surface` stage (blocked on JNI `Surface` sourcing, a headless Rust library has no
//!   Activity context); see [`docs/roadmap.md`](../docs/roadmap.md).
//!
//! Policy: [ADR android/0001](../adr/android/0001-ndk-amediacodec-h264-cpu-out.md) — binding
//! choice (`ndk`), scope, CSD (`csd-0`/`csd-1`) handoff, and the **zero compile verification
//! and zero runtime verification** caveat for this crate as authored (this dev environment has
//! no Android NDK and no device/emulator). Read that caveat before relying on this backend.

// Unlike `mediaway-decoder-windows`, this crate writes no raw FFI `unsafe` of its own: all
// `AMediaCodec` calls go through `ndk::media::media_codec`'s safe wrapper layer. See ADR
// android/0001 for why that safe surface is sufficient for this crate's scope — mirrors
// `mediaway-encoder-android`'s identical choice.
#![forbid(unsafe_code)]

#[cfg(not(feature = "video"))]
compile_error!("enable the `video` feature on mediaway-decoder-android");

use crate::DecodeError;
#[cfg(feature = "video")]
use crate::{VideoDecoder, VideoDecoderConfig};
#[cfg(feature = "video")]
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

// Not gated to `target_os = "android"` at the module-declaration level (unlike
// `linux::vaapi`'s single `#[cfg(target_os = "linux")] mod vaapi;`): `amediacodec::nv12` and
// `amediacodec::csd` are pure byte-manipulation helpers with no `ndk` dependency, kept
// host-testable on any platform (this crate's dev environment has no Android NDK/device) per
// this backend's unit-test coverage requirement; only `amediacodec::video` (the real
// `MediaCodec` session) is internally gated to `target_os = "android"`. See
// `amediacodec/mod.rs` for the exact per-submodule gating.
mod amediacodec;

/// Android video decode session (NDK `AMediaCodec` H.264 when opened on Android).
#[cfg(feature = "video")]
pub struct AndroidVideoDecoder {
    #[cfg(target_os = "android")]
    inner: Option<amediacodec::AmediaCodecVideoDecoder>,
    #[cfg(not(target_os = "android"))]
    _priv: (),
}

#[cfg(feature = "video")]
impl AndroidVideoDecoder {
    /// Open an Android `AMediaCodec` video decoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Unsupported`] when the codec/output path is not wired
    /// (currently: anything but H.264 + [`VideoOutputPreference::CpuFramesOk`]), or
    /// [`DecodeError::Backend`] when no matching decoder is available on-device (the Android
    /// CDD requires at least one AVC decoder, but this crate does not assume a specific one
    /// exists) or `configure`/`start` fails — see ADR android/0001.
    ///
    /// [`VideoOutputPreference::CpuFramesOk`]: crate::VideoOutputPreference::CpuFramesOk
    #[cfg(target_os = "android")]
    pub fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        let inner = amediacodec::AmediaCodecVideoDecoder::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Android build: decoder unavailable.
    #[cfg(not(target_os = "android"))]
    pub const fn open(_config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        Err(DecodeError::Unsupported)
    }
}

#[cfg(feature = "video")]
#[cfg(target_os = "android")]
impl VideoDecoder for AndroidVideoDecoder {
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
#[cfg(not(target_os = "android"))]
impl VideoDecoder for AndroidVideoDecoder {
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
