//! `mediaway_auto_video_encode_config_t` constructors.
//!
//! Both are safe, pure value construction — no pointer params, cannot panic,
//! so no `catch_unwind` is needed (`adr/0001-auto-encode-c-abi.md` §4).

use crate::types::{
    MediawayAudioEncodeConfig, MediawayAutoVideoEncodeConfig, MediawayGpuDeviceHandle,
    MediawayGpuDeviceKind, MediawayPipelineCodecKind, MediawayPixelFormat, MediawayRational,
    MediawaySampleFormat,
};

/// Build a config for `codec` at `width`x`height`/`time_base`.
///
/// Defaults `bitrate_bps` to `0` (backend default), `pixel_format` to NV12, and
/// `gpu_device` to `None` (CPU-only), matching `AutoVideoEncodeConfig::new`'s own
/// Rust defaults. Callers wanting the GPU input path set `.gpu_device` on the
/// returned value afterward.
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_auto_video_encode_config_new(
    codec: MediawayPipelineCodecKind,
    width: u32,
    height: u32,
    time_base: MediawayRational,
) -> MediawayAutoVideoEncodeConfig {
    MediawayAutoVideoEncodeConfig {
        codec,
        width,
        height,
        time_base,
        bitrate_bps: 0,
        pixel_format: MediawayPixelFormat::Nv12,
        gpu_device: MediawayGpuDeviceHandle {
            kind: MediawayGpuDeviceKind::None,
            native: 0,
            webgpu_device_id: 0,
        },
    }
}

/// Sugar over [`mediaway_auto_video_encode_config_new`] with
/// `codec = MEDIAWAY_PIPELINE_CODEC_H264`.
///
/// Kept because the aspirational example calls it, but v1 also exposes the
/// general form since H.264 is not the only real codec the Windows auto
/// backend resolves today.
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_auto_video_encode_config_h264(
    width: u32,
    height: u32,
    time_base: MediawayRational,
) -> MediawayAutoVideoEncodeConfig {
    mediaway_auto_video_encode_config_new(MediawayPipelineCodecKind::H264, width, height, time_base)
}

/// Build an AAC audio encode config (`adr/0003-auto-audio-encode-c-abi.md`):
/// stereo, F32 input, backend-default bitrate.
///
/// Mirrors [`mediaway_audio_encode_config_aac`]'s sugar role — the general
/// form (any codec/format) exists as the struct itself, so no general
/// constructor is exported yet.
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_audio_encode_config_aac(
    sample_rate: u32,
    time_base: MediawayRational,
) -> MediawayAudioEncodeConfig {
    MediawayAudioEncodeConfig {
        codec: MediawayPipelineCodecKind::Aac,
        sample_rate,
        channels: 2,
        sample_format: MediawaySampleFormat::F32,
        time_base,
        bitrate_bps: 0,
    }
}
