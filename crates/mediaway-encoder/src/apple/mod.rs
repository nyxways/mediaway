//! Apple (macOS + iOS) encode backend (`VideoToolbox` `VTCompressionSession`, via `objc2`).
//!
//! - [`VideoInputPreference::CpuUploadOk`](crate::VideoInputPreference): H.264/HEVC session via
//!   `VTCompressionSession` + `upload_cpu_nv12` (`CVPixelBufferCreateWithPlanarBytes`, one
//!   copy) — `kVTCompressionPropertyKey_MaxKeyFrameInterval` requests
//!   [`VideoEncoderConfig::gop_size`] sync frames, device-dependent, not a byte-exact
//!   guarantee like Linux's raw bitstream approach. VP9/AV1 are **not** supported — `VideoToolbox`
//!   exposes no compression API for either codec at all (see ADR-0002).
//! - [`VideoInputPreference::ZeroCopyGpu`]: real Zero-Copy — the caller's
//!   [`GpuBufferHandle::Metal`](mediaway_common::GpuBufferHandle::Metal) `CVPixelBuffer` is
//!   borrowed directly for `VTCompressionSession::encode_frame`, never copied or retained by
//!   this backend; see [ADR-0003](../adr/apple/0003-videotoolbox-metal-zero-copy-encode.md).
//!
//! Policy: [ADR-0001](../adr/apple/0001-videotoolbox-h264-cpu-upload.md) — original binding
//! choice (`objc2-video-toolbox`/`objc2-core-video`/`objc2-core-media`/`objc2-core-foundation`)
//! and H.264 scope. [ADR-0002](../adr/apple/0002-videotoolbox-hevc-encode.md) — HEVC addition and
//! VP9/AV1's permanent non-support. [ADR-0003](../adr/apple/0003-videotoolbox-metal-zero-copy-encode.md)
//! — Zero-Copy input. All three carry the **zero compile verification as authored** caveat (this
//! crate's dev environment cannot cross-compile Apple code at all — no Xcode/Apple SDK reachable
//! outside macOS). Read that caveat before relying on this backend.

// Unlike Android/Linux, VideoToolbox/CoreMedia/CoreVideo's `objc2-*` bindings are plain C-API
// `unsafe fn` wrappers with no safe layer (see ADR-0001 § Unsafe surface) — this module carries
// real `unsafe` blocks with `// SAFETY:` comments, matching `src/windows/`'s discipline. The
// crate root's `#![allow(unsafe_code)]` (see `lib.rs`) applies here.

#[cfg(all(not(feature = "audio"), not(feature = "video")))]
compile_error!("enable the `audio` and/or `video` feature on mediaway-encoder-apple");

use crate::EncodeError;
#[cfg(feature = "audio")]
use crate::{AudioEncoder, AudioEncoderConfig};
use crate::{VideoEncoder, VideoEncoderConfig};
#[cfg(feature = "audio")]
use mediaway_common::AudioFrame;
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

#[cfg(all(any(target_os = "macos", target_os = "ios"), feature = "audio"))]
mod audiotoolbox;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod videotoolbox;

/// Apple video encode session (`VideoToolbox` H.264/HEVC when opened on macOS/iOS).
pub struct AppleVideoEncoder {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    inner: Option<videotoolbox::VideoToolboxVideoEncoder>,
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    _priv: (),
}

impl AppleVideoEncoder {
    /// Open an Apple `VideoToolbox` video encoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] when the codec is not H.264/HEVC (VP9/AV1 have no
    /// `VideoToolbox` compression API at all, see ADR-0002), or [`EncodeError::Backend`]
    /// when `VTCompressionSessionCreate`/`VTSessionSetProperty` fails (a real, honest failure —
    /// not every device has a given codec's HW encoder available — see ADR-0001). Returns
    /// [`EncodeError::InvalidInput`] at `push_frame` time when a frame's storage doesn't match
    /// the `input` preference `open` was called with (see ADR-0003).
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let inner = videotoolbox::VideoToolboxVideoEncoder::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Apple build: encoder unavailable.
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pub const fn open(_config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl VideoEncoder for AppleVideoEncoder {
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

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
impl VideoEncoder for AppleVideoEncoder {
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

/// Apple audio encode session.
///
/// AAC via `AudioConverter` ([ADR-0004](../adr/apple/0004-audiotoolbox-aac-encode.md)) or Opus
/// via `mediaway-sw` (cross-platform, no Apple-specific code), dispatched by
/// [`AudioEncoderConfig::codec`] — mirrors
/// `mediaway-encoder::windows::WindowsAudioEncoder`'s identical `AudioBackend` shape.
#[cfg(feature = "audio")]
pub struct AppleAudioEncoder {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    inner: Option<AudioBackend>,
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    _priv: (),
}

/// Per-codec audio backend behind [`AppleAudioEncoder`].
#[cfg(all(feature = "audio", any(target_os = "macos", target_os = "ios")))]
enum AudioBackend {
    /// `AudioConverter` AAC-LC encoder.
    Aac(audiotoolbox::AacEncoder),
    /// Software Opus encoder (`unsafe-libopus` via `mediaway-sw`) — no `VideoToolbox`/
    /// `AudioToolbox` Opus encoder exists.
    Opus(crate::SwOpusAudioEncoder),
}

#[cfg(feature = "audio")]
impl AppleAudioEncoder {
    /// Open an Apple audio encoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] for any codec but AAC/Opus, or
    /// [`EncodeError::Backend`] on `AudioConverter`/`unsafe-libopus` failure.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn open(config: &AudioEncoderConfig) -> Result<Self, EncodeError> {
        let inner = match config.codec {
            mediaway_common::CodecKind::Aac => {
                AudioBackend::Aac(audiotoolbox::AacEncoder::open(config)?)
            }
            mediaway_common::CodecKind::Opus => {
                AudioBackend::Opus(crate::SwOpusAudioEncoder::open(config)?)
            }
            _ => return Err(EncodeError::Unsupported),
        };
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Apple build: encoder unavailable.
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pub const fn open(_config: &AudioEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(all(feature = "audio", any(target_os = "macos", target_os = "ios")))]
impl AudioEncoder for AppleAudioEncoder {
    fn stream_info(&self) -> &StreamInfo {
        match self.inner.as_ref() {
            Some(AudioBackend::Aac(e)) => e.stream_info(),
            Some(AudioBackend::Opus(e)) => e.stream_info(),
            None => closed_audio_stream_info(),
        }
    }

    fn push_frame(&mut self, frame: &AudioFrame) -> Result<(), EncodeError> {
        match self.inner.as_mut() {
            Some(AudioBackend::Aac(e)) => e.push_frame(frame),
            Some(AudioBackend::Opus(e)) => e.push_frame(frame),
            None => Err(EncodeError::Closed),
        }
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        match self.inner.as_mut() {
            Some(AudioBackend::Aac(e)) => e.poll_packet(),
            Some(AudioBackend::Opus(e)) => e.poll_packet(),
            None => Err(EncodeError::Closed),
        }
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        match self.inner.as_mut() {
            Some(AudioBackend::Aac(e)) => e.flush(),
            Some(AudioBackend::Opus(e)) => e.flush(),
            None => Err(EncodeError::Closed),
        }
    }
}

#[cfg(all(feature = "audio", not(any(target_os = "macos", target_os = "ios"))))]
impl AudioEncoder for AppleAudioEncoder {
    fn stream_info(&self) -> &StreamInfo {
        closed_audio_stream_info()
    }

    fn push_frame(&mut self, _frame: &AudioFrame) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(None)
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(feature = "audio")]
fn closed_audio_stream_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Audio {
        id: 0,
        codec: mediaway_common::CodecKind::Aac,
        time_base: mediaway_common::Rational::new(1, 48_000),
        extra_data: Bytes::new(),
        sample_rate: 0,
        channels: 0,
    })
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

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
#[path = "lib_tests.rs"]
mod tests;
