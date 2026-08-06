//! C ABI facade over [`mediaway`] (auto video/audio encode -> fragmented MP4, auto
//! video decode, and Opus audio decode).
//!
//! Design: `adr/0001-auto-encode-c-abi.md` — opaque handles (`AutoEncoderHandle`
//! needs no `poisoned` flag; `EncodeSessionHandle` does), a 15-value
//! `mediaway_pipeline_status_t`, `catch_unwind` panic safety, and a hand-written
//! header (`include/mediaway/pipeline.h`). Design rules:
//! [`docs/spec/c-ffi.md`](../../../../docs/spec/c-ffi.md) (ADR-0004).
//!
//! This crate's `pipeline` module (formerly the standalone `mediaway-ffi` crate,
//! merged by ADR-0021). Wraps `mediaway`'s `platform::AutoEncoder` + `EncodeSession`
//! (auto-selected OS/GPU encoder wired straight into a fragmented-MP4 muxer),
//! `platform::AutoDecoder` (`adr/0004-auto-decode-c-abi.md`), `AudioEncoder`
//! (`adr/0003-auto-audio-encode-c-abi.md`, AAC on Windows + cross-platform Opus via
//! `mediaway-sw`), and `mediaway_sw::opus::OpusDecoder`
//! (`adr/pipeline/0006-audio-decode-c-abi.md`) over a C ABI: opaque handles + integer
//! error codes, no panics/unwinding across the boundary.
//!
//! `gpu_device` is reachable from C for encode input (`adr/0002-gpu-frame-input-c-abi.md`),
//! opening the Zero-Copy GPU encode path end-to-end. `backend`/`max_path_class` stay
//! deferred (`adr/0001-auto-encode-c-abi.md` §1) — `AutoVideoEncodeConfig`'s own
//! `max_path_class` default already permits `ZeroCopy`/`GpuCopy`. Decode output stays
//! CPU-only (`adr/0004-auto-decode-c-abi.md` §1).

#![allow(unsafe_code)] // FFI crate — see docs/conventions/code-style.md § unsafe

mod audio;
mod audio_decoder;
mod buffer;
#[cfg(any(feature = "camera", feature = "desktop"))]
mod capture_bridge;
mod config;
mod decoder;
mod encoder;
mod session;
mod status;
mod types;

pub use audio::{
    AudioEncodeSessionHandle, mediaway_audio_encode_session_close,
    mediaway_audio_encode_session_flush, mediaway_audio_encode_session_poll_packet,
    mediaway_audio_encode_session_push_pcm, mediaway_audio_encode_session_stream_info,
    mediaway_audio_encoder_open, mediaway_pipeline_ffi_packet_free,
    mediaway_pipeline_ffi_stream_info_free,
};
pub use audio_decoder::{
    AudioDecodeSessionHandle, mediaway_audio_decode_session_close,
    mediaway_audio_decode_session_flush, mediaway_audio_decode_session_open,
    mediaway_audio_decode_session_poll_frame, mediaway_audio_decode_session_push_packet,
    mediaway_decoded_audio_frame_free,
};
pub use buffer::mediaway_pipeline_ffi_buffer_free;
#[cfg(feature = "camera")]
pub use capture_bridge::mediaway_encode_session_write_frame_from_camera_capture;
#[cfg(feature = "desktop")]
pub use capture_bridge::mediaway_encode_session_write_frame_from_desktop_capture;
pub use config::{
    mediaway_audio_decode_config_opus, mediaway_audio_encode_config_aac,
    mediaway_audio_encode_config_opus, mediaway_auto_video_decode_config_new,
    mediaway_auto_video_encode_config_h264, mediaway_auto_video_encode_config_new,
};
pub use decoder::{
    DecodeSessionHandle, mediaway_decode_session_close, mediaway_decode_session_flush,
    mediaway_decode_session_open, mediaway_decode_session_poll_frame,
    mediaway_decode_session_push_packet, mediaway_decoded_video_frame_free,
};
pub use encoder::{AutoEncoderHandle, mediaway_auto_encoder_close, mediaway_auto_encoder_open};
pub use session::{
    EncodeSessionHandle, mediaway_encode_session_close, mediaway_encode_session_finish,
    mediaway_encode_session_open, mediaway_encode_session_set_bitrate,
    mediaway_encode_session_write_frame,
};
pub use status::MediawayPipelineStatus;
pub use types::{
    MediawayAudioDecodeConfig, MediawayAudioEncodeConfig, MediawayAudioFrameView,
    MediawayAudioPacket, MediawayAudioStreamInfo, MediawayAutoVideoDecodeConfig,
    MediawayAutoVideoEncodeConfig, MediawayDecodePacketView, MediawayDecodedAudioFrame,
    MediawayDecodedVideoFrame, MediawayGpuBufferHandle, MediawayGpuBufferKind,
    MediawayGpuDeviceHandle, MediawayGpuDeviceKind, MediawayPipelineCodecKind, MediawayPixelFormat,
    MediawayRational, MediawaySampleFormat, MediawayVideoFrame, MediawayVideoFrameStorageKind,
};

/// Runtime ABI version, matching `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` in
/// `include/mediaway/pipeline.h`.
///
/// A dynamically-loaded consumer (Python/Node/Go/...) that never compiles against the
/// header can call this to assert the loaded library matches what it was built against.
///
/// Bumped `0` -> `1`: `mediaway_auto_video_encode_config_t` and `mediaway_video_frame_t`
/// both gained new fields, a breaking layout change (`adr/0002-gpu-frame-input-c-abi.md`).
/// Bumped `1` -> `2`: added the audio encode surface
/// (`adr/0003-auto-audio-encode-c-abi.md`).
/// Bumped `2` -> `3`: added the video decode surface
/// (`adr/0004-auto-decode-c-abi.md`).
/// Bumped `3` -> `4`: added the capture-to-encode bridge
/// (`adr/0005-capture-encode-bridge-c-abi.md`).
/// Bumped `4` -> `5`: added the Opus audio decode surface and wired `CodecKind::Opus`
/// into the existing audio encode surface (`adr/pipeline/0006-audio-decode-c-abi.md`).
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_pipeline_ffi_abi_version() -> u32 {
    5
}
