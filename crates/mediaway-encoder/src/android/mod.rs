//! Android encode backend (NDK `AMediaCodec`, via the `ndk` crate).
//!
//! - [`VideoInputPreference::CpuUploadOk`](crate::VideoInputPreference): H.264 (`video/avc`)
//!   session via `ndk::media::media_codec::MediaCodec` + `upload_cpu_yuv420` (copy) —
//!   `KEY_I_FRAME_INTERVAL` is set to request every frame as a sync frame, but (unlike the
//!   Linux VA-API backend's raw per-frame IDR bitstream) this is a device-dependent codec
//!   behavior, not a byte-exact guarantee — see ADR-0001 § Consequences.
//! - [`VideoInputPreference::ZeroCopyGpu`]: not implemented — returns
//!   [`EncodeError::Unsupported`]. Deferred to a Zero-Copy `AHardwareBuffer`/`ANativeWindow`
//!   stage; see [`docs/roadmap.md`](../docs/roadmap.md).
//!
//! Policy: [ADR-0001](../adr/android/0001-ndk-amediacodec-h264-cpu-upload.md) — binding choice
//! (`ndk`), scope, and the **zero compile verification as authored** caveat (this crate's dev
//! environment has no Android NDK; see the ADR's § CI verification plan). Read that caveat
//! before relying on this backend.

// Unlike `mediaway-encoder-windows`, this crate writes no raw FFI `unsafe` of its own: all
// `AMediaCodec` calls go through `ndk::media::media_codec`'s safe wrapper layer. See ADR-0001
// for why that safe surface is sufficient for this crate's scope.
#![forbid(unsafe_code)]

#[cfg(not(feature = "video"))]
compile_error!("enable the `video` feature on mediaway-encoder-android");

use crate::EncodeError;
use crate::{VideoEncoder, VideoEncoderConfig};
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

#[cfg(target_os = "android")]
mod amediacodec;

/// Android video encode session (NDK `AMediaCodec` H.264 when opened on Android).
pub struct AndroidVideoEncoder {
    #[cfg(target_os = "android")]
    inner: Option<amediacodec::AmediaCodecVideoEncoder>,
    #[cfg(not(target_os = "android"))]
    _priv: (),
}

impl AndroidVideoEncoder {
    /// Open an Android `AMediaCodec` video encoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] when the codec/input path is not wired
    /// (currently: anything but H.264 + [`VideoInputPreference::CpuUploadOk`]), or
    /// [`EncodeError::Backend`] when no matching encoder is available on-device (a real,
    /// honest failure — not every AOSP device is guaranteed a given codec name, though the
    /// Android CDD requires at least one AVC encoder) or `configure`/`start` fails — see
    /// ADR-0001.
    ///
    /// [`VideoInputPreference::CpuUploadOk`]: crate::VideoInputPreference::CpuUploadOk
    #[cfg(target_os = "android")]
    pub fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let inner = amediacodec::AmediaCodecVideoEncoder::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Android build: encoder unavailable.
    #[cfg(not(target_os = "android"))]
    pub const fn open(_config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(target_os = "android")]
impl VideoEncoder for AndroidVideoEncoder {
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

#[cfg(not(target_os = "android"))]
impl VideoEncoder for AndroidVideoEncoder {
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

#[cfg(all(test, target_os = "android"))]
#[path = "lib_tests.rs"]
mod tests;
