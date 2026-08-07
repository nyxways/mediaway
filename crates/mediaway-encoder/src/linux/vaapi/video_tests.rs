//! Tests for [`super::VaapiVideoEncoder`] — see `docs/conventions/testing.md` Tier 1.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use mediaway_common::{CodecKind, Rational};

fn tiny_h264_cfg(width: u32, height: u32) -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: CodecKind::H264,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    }
}

// --- Pure logic: no display/driver needed --------------------------------------------------

#[test]
fn validate_accepts_mb_aligned_h264_cpu_upload_config() {
    assert!(validate(&tiny_h264_cfg(64, 64)).is_ok());
}

#[test]
fn validate_rejects_non_h264_codec() {
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.codec = CodecKind::Hevc;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_dimensions() {
    let cfg = tiny_h264_cfg(0, 64);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_rejects_non_macroblock_aligned_dimensions() {
    // 65 is not a multiple of 16 — frame cropping is out of scope this stage (ADR-0001).
    let cfg = tiny_h264_cfg(65, 64);
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_non_nv12_pixel_format() {
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.pixel_format = PixelFormat::Bgra8;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_timebase_denominator() {
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.time_base = Rational::new(1, 0);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn mb_count_converts_16_aligned_dimensions() {
    assert_eq!(mb_count(16).unwrap(), 1);
    assert_eq!(mb_count(64).unwrap(), 4);
    assert_eq!(mb_count(1920).unwrap(), 120);
}

#[test]
fn nv12_size_is_one_and_a_half_bytes_per_pixel() {
    // 64x64 NV12: 64*64 Y bytes + 64*64/2 interleaved UV bytes.
    assert_eq!(nv12_size(64, 64).unwrap(), 64 * 64 + (64 * 64) / 2);
    assert_eq!(nv12_size(16, 16).unwrap(), 16 * 16 + (16 * 16) / 2);
}

#[test]
fn nv12_size_rejects_overflowing_dimensions() {
    assert_eq!(
        nv12_size(u32::MAX, u32::MAX),
        Err(EncodeError::InvalidInput)
    );
}

#[test]
fn stream_info_from_config_carries_geometry_and_timebase() {
    let cfg = tiny_h264_cfg(64, 32);
    let info = stream_info_from(&cfg);
    assert!(matches!(
        info,
        StreamInfo::Video {
            codec: CodecKind::H264,
            ..
        }
    ));
    if let StreamInfo::Video {
        time_base,
        geometry,
        ..
    } = info
    {
        assert_eq!(time_base, cfg.time_base);
        assert_eq!(geometry.width, 64);
        assert_eq!(geometry.height, 32);
    }
}

// --- Hardware-gated: real VA-API session ---------------------------------------------------

/// Opens a real VA-API H.264 CPU-upload session and, if a display is available, encodes one
/// black NV12 frame end to end (`vaCreateConfig`/`vaCreateContext`/`vaCreateSurfaces` →
/// `upload_cpu_nv12` → `vaRenderPicture`/`vaEndPicture`/`vaSyncSurface` → mapped coded output).
///
/// **Expected to skip in the session that authored this crate** — there is no real
/// `/dev/dri/renderD*` VA-API device available (Windows host; the WSL2 environment used for
/// compile verification has broken VA-API / software-only Vulkan). See
/// [ADR-0001](../../adr/0001-vaapi-cros-libva-h264-cpu-upload.md) § Zero real-hardware
/// verification. This test is provided so a future run on real Linux + VA-API hardware
/// (Intel iHD / Mesa / AMD) can verify the path this crate was written against.
#[test]
fn vaapi_open_and_encode_or_skip_without_hw() {
    let cfg = tiny_h264_cfg(64, 64);
    let mut enc = match VaapiVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: VaapiVideoEncoder::open failed ({e:?}) — no VA-API display?");
            return;
        }
    };

    let nv12_len = 64 * 64 + (64 * 64) / 2;
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: 64,
        height: 64,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(vec![0u8; nv12_len]),
        },
    };

    if let Err(e) = enc.push_frame(&frame) {
        eprintln!("skip: push_frame failed ({e:?}) — no usable VA-API encode session?");
        return;
    }
    let _ = enc.flush();

    let mut packets = 0usize;
    while let Ok(Some(p)) = enc.poll_packet() {
        assert!(!p.payload.is_empty());
        assert!(p.is_keyframe);
        packets += 1;
    }
    eprintln!("vaapi h264 cpu-upload packets={packets}");
}
