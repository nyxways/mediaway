//! Apple (macOS + iOS) decode backend (`VideoToolbox` `VTDecompressionSession`, via `objc2`).
//!
//! - [`VideoOutputPreference::CpuFramesOk`](crate::VideoOutputPreference): H.264/HEVC/VP9/AV1
//!   decode through `VTDecompressionSession` (general GOP — `VideoToolbox` owns the DPB and
//!   P/B-frame reorder via `kVTDecodeFrame_EnableTemporalProcessing`) with CPU NV12
//!   (`VideoRange`) readback inside the decompression output callback
//!   (`CVPixelBufferLockBaseAddress` + a stride-aware plane copy) — no `CVPixelBuffer`/
//!   `IOSurface` export, so this is honest CPU decode, not a disguised Zero-Copy path.
//! - [`VideoOutputPreference::ZeroCopyGpu`]: real Zero-Copy — a **new**, independent
//!   `CFRetain` on the decoded `CVPixelBuffer` (`CFRetained::retain`, never a lock/plane-byte
//!   read) is handed out as
//!   [`GpuBufferHandle::Metal`](mediaway_common::GpuBufferHandle::Metal); valid until the next
//!   `push_packet`/`poll_frame`/`flush` call, which releases it. See
//!   [ADR-0003](../adr/apple/0003-videotoolbox-metal-zero-copy-decode.md).
//!
//! Policy: [ADR-0001](../adr/apple/0001-videotoolbox-h264-cpu-out.md) — original binding choice
//! (`objc2-video-toolbox`/`objc2-core-media`/`objc2-core-video`/`objc2-core-foundation`), H.264
//! scope (one SPS + one PPS, 4-byte AVCC length size only), byte framing (reuses
//! `iso_bmff::bitstream::avc` both directions). [ADR-0002](../adr/apple/0002-videotoolbox-hevc-vp9-av1-decode.md)
//! — HEVC/VP9/AV1 multicodec expansion. [ADR-0003](../adr/apple/0003-videotoolbox-metal-zero-copy-decode.md)
//! — Zero-Copy output. All three carry the **zero compile verification as authored** caveat
//! (this crate's dev environment cannot cross-compile Apple code at all — no Xcode/Apple SDK
//! reachable outside macOS). Read that caveat before relying on this backend.

// Unlike `linux::vaapi` (which forbids unsafe entirely — see that module's own doc comment),
// VideoToolbox/CoreMedia/CoreVideo's `objc2-*` bindings are plain C-API `unsafe fn` wrappers
// with no safe layer, matching `src/windows/`'s and `mediaway-encoder::apple`'s discipline: real
// `unsafe` blocks with `// SAFETY:` comments, real code only on Apple targets, `deny`d on every
// other target (nothing there ever needs it).
#![cfg_attr(any(target_os = "macos", target_os = "ios"), allow(unsafe_code))]
#![cfg_attr(not(any(target_os = "macos", target_os = "ios")), deny(unsafe_code))]

#[cfg(all(not(feature = "audio"), not(feature = "video")))]
compile_error!("enable the `audio` and/or `video` feature on mediaway-decoder-apple");

use crate::DecodeError;
#[cfg(feature = "video")]
use crate::{VideoDecoder, VideoDecoderConfig};
#[cfg(feature = "video")]
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

// Gated to real Apple targets for actual use, plus `cfg(test)` so `videotoolbox::codec`'s pure
// helpers (no `objc2-*` import — see that module's own doc comment) build and unit-test on this
// crate's non-Apple CI/dev hosts too. A child `mod` cannot survive a cfg-false parent, so this
// gate must include `test` even though `videotoolbox::video` itself stays Apple-only.
#[cfg(any(target_os = "macos", target_os = "ios", test))]
mod videotoolbox;

#[cfg(all(any(target_os = "macos", target_os = "ios"), feature = "audio"))]
mod audiotoolbox;
/// Reachable as `mediaway_decoder::apple::AacDecoder` — see
/// [ADR-0004](../adr/apple/0004-audiotoolbox-aac-decode.md). No `AppleAudioDecoder` wrapper
/// exists (mirrors `mediaway-decoder::windows`'s own `WmfOpusDecoder` exposure — no
/// `WindowsAudioDecoder` wrapper exists there either).
#[cfg(all(any(target_os = "macos", target_os = "ios"), feature = "audio"))]
pub use audiotoolbox::{AacDecoder, AacDecoderConfig};

/// Apple video decode session (`VideoToolbox` H.264 when opened on macOS/iOS).
#[cfg(feature = "video")]
pub struct AppleVideoDecoder {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    inner: Option<videotoolbox::VideoToolboxVideoDecoder>,
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    _priv: (),
}

#[cfg(feature = "video")]
impl AppleVideoDecoder {
    /// Open an Apple `VideoToolbox` video decoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Unsupported`] when the codec is not H.264/HEVC/VP9/AV1, or VP9/AV1
    /// lack a container-supplied config record (see `codec::requires_extra_data_at_open`), or
    /// [`DecodeError::Backend`] on `VTDecompressionSessionCreate`/
    /// `CMVideoFormatDescriptionCreate*` failure.
    ///
    /// [`VideoOutputPreference::CpuFramesOk`]: crate::VideoOutputPreference::CpuFramesOk
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        let inner = videotoolbox::VideoToolboxVideoDecoder::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Apple build: decoder unavailable.
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pub const fn open(_config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        Err(DecodeError::Unsupported)
    }
}

#[cfg(feature = "video")]
#[cfg(any(target_os = "macos", target_os = "ios"))]
impl VideoDecoder for AppleVideoDecoder {
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
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
impl VideoDecoder for AppleVideoDecoder {
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
