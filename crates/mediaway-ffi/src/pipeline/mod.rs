//! C ABI facade over [`mediaway_pipeline`] (auto video encode -> fragmented MP4).
//!
//! Design: `adr/0001-auto-encode-c-abi.md` — opaque handles (`AutoEncoderHandle`
//! needs no `poisoned` flag; `EncodeSessionHandle` does), a 13-value
//! `mediaway_pipeline_status_t`, `catch_unwind` panic safety, and a hand-written
//! header (`include/mediaway/pipeline.h`). Design rules:
//! [`docs/spec/c-ffi.md`](../../../../docs/spec/c-ffi.md) (ADR-0004).
//!
//! Second `mediaway-*-ffi` crate in the workspace, after `mediaway-container-ffi`
//! ([`docs/spec/crate-packaging.md`](../../../../docs/spec/crate-packaging.md)). Wraps
//! `mediaway-pipeline`'s `platform::AutoEncoder` + `EncodeSession` (auto-selected OS/GPU
//! encoder wired straight into a fragmented-MP4 muxer) over a C ABI: opaque handles +
//! integer error codes, no panics/unwinding across the boundary.
//!
//! `gpu_device` is now reachable from C (`adr/0002-gpu-frame-input-c-abi.md`),
//! opening the Zero-Copy GPU encode path end-to-end. `backend`/`max_path_class`
//! stay deferred (`adr/0001-auto-encode-c-abi.md` §1) — `AutoVideoEncodeConfig`'s
//! own `max_path_class` default already permits `ZeroCopy`/`GpuCopy`.

#![allow(unsafe_code)] // FFI crate — see docs/conventions/code-style.md § unsafe

mod audio;
mod buffer;
mod config;
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
pub use buffer::mediaway_pipeline_ffi_buffer_free;
pub use config::{
    mediaway_audio_encode_config_aac, mediaway_auto_video_encode_config_h264,
    mediaway_auto_video_encode_config_new,
};
pub use encoder::{AutoEncoderHandle, mediaway_auto_encoder_close, mediaway_auto_encoder_open};
pub use session::{
    EncodeSessionHandle, mediaway_encode_session_close, mediaway_encode_session_finish,
    mediaway_encode_session_open, mediaway_encode_session_write_frame,
};
pub use status::MediawayPipelineStatus;
pub use types::{
    MediawayAudioEncodeConfig, MediawayAudioFrameView, MediawayAudioPacket,
    MediawayAudioStreamInfo, MediawayAutoVideoEncodeConfig, MediawayGpuBufferHandle,
    MediawayGpuBufferKind, MediawayGpuDeviceHandle, MediawayGpuDeviceKind,
    MediawayPipelineCodecKind, MediawayPixelFormat, MediawayRational, MediawaySampleFormat,
    MediawayVideoFrame, MediawayVideoFrameStorageKind,
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
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_pipeline_ffi_abi_version() -> u32 {
    2
}
