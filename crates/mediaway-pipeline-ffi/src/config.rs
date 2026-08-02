//! `mediaway_auto_video_encode_config_t` constructors.
//!
//! Both are safe, pure value construction — no pointer params, cannot panic,
//! so no `catch_unwind` is needed (`adr/0001-auto-encode-c-abi.md` §4).

use crate::types::{
    MediawayAutoVideoEncodeConfig, MediawayGpuDeviceHandle, MediawayGpuDeviceKind,
    MediawayPipelineCodecKind, MediawayPixelFormat, MediawayRational,
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
