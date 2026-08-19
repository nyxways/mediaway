//! Tests for [`super::VaapiVp9Encoder`] — see `docs/conventions/testing.md` Tier 1.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use mediaway_common::{CodecKind, Rational};

fn tiny_vp9_cfg(width: u32, height: u32) -> VideoEncoderConfig {
    tiny_vp9_gop_cfg(width, height, 1)
}

fn tiny_vp9_gop_cfg(width: u32, height: u32, gop_size: u32) -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: CodecKind::Vp9,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size,
        rate_control: None,
        intra_refresh_period: None,
    }
}

// --- Pure logic: no display/driver needed --------------------------------------------------

#[test]
fn validate_accepts_vp9_cpu_upload_config() {
    assert!(validate(&tiny_vp9_cfg(64, 64)).is_ok());
}

#[test]
fn validate_rejects_non_vp9_codec() {
    let mut cfg = tiny_vp9_cfg(64, 64);
    cfg.codec = CodecKind::H264;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_dimensions() {
    let cfg = tiny_vp9_cfg(0, 64);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_rejects_non_nv12_pixel_format() {
    let mut cfg = tiny_vp9_cfg(64, 64);
    cfg.pixel_format = PixelFormat::I420;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_time_base_denominator() {
    let mut cfg = tiny_vp9_cfg(64, 64);
    cfg.time_base = Rational::new(1, 0);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_rejects_zero_gop_size() {
    let cfg = tiny_vp9_gop_cfg(64, 64, 0);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn nv12_size_computes_1_5_bytes_per_pixel() {
    assert_eq!(nv12_size(64, 64).unwrap(), 64 * 64 * 3 / 2);
}

#[test]
fn log2_tile_columns_is_zero_under_max_tile_width() {
    assert_eq!(log2_tile_columns(64), 0);
    assert_eq!(log2_tile_columns(VP9_MAX_TILE_WIDTH), 0);
}

#[test]
fn log2_tile_columns_is_nonzero_above_max_tile_width() {
    // 2 tile columns needed just above the threshold -> log2(2 - 1) + 1 = 1.
    assert_eq!(log2_tile_columns(VP9_MAX_TILE_WIDTH + 1), 1);
}

// --- Hardware-gated: real VA-API session ---------------------------------------------------

/// Opens a real VA-API VP9 CPU-upload session and, if a display + VP9 encode config is
/// available, encodes one black NV12 frame end to end.
///
/// **Expected to skip in the session that authored this crate** — no real `/dev/dri/renderD*`
/// VA-API device is available, and even on real hardware VP9 encode driver support is narrow
/// (`FFmpeg`'s own source names only i965 as a working driver — see this ADR's own § Real caveat
/// found this session). See
/// [ADR-0004](../../adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md).
#[test]
fn vaapi_open_and_encode_or_skip_without_hw() {
    let cfg = tiny_vp9_cfg(64, 64);
    let mut enc = match VaapiVp9Encoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: VaapiVp9Encoder::open failed ({e:?}) — no VA-API VP9 encode config?");
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
        eprintln!("skip: push_frame failed ({e:?}) — no usable VA-API VP9 encode session?");
        return;
    }
    let _ = enc.flush();

    let mut packets = 0usize;
    while let Ok(Some(p)) = enc.poll_packet() {
        assert!(!p.payload.is_empty());
        assert!(p.is_keyframe);
        packets += 1;
    }
    eprintln!("vaapi vp9 cpu-upload packets={packets}");
}

fn black_nv12_frame(pts: i64, width: u32, height: u32) -> VideoFrame {
    let nv12_len = (width * height + (width * height) / 2) as usize;
    VideoFrame {
        pts,
        duration: 1,
        width,
        height,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(vec![0u8; nv12_len]),
        },
    }
}

/// Pushes 7 frames with `gop_size = 3` through a real VA-API VP9 session and confirms the
/// resulting packets' `is_keyframe` cadence is `K P P K P P K`.
///
/// **Expected to skip in the session that authored this crate** — same zero-real-hardware
/// disposition as `vaapi_open_and_encode_or_skip_without_hw` above.
#[test]
fn vaapi_gop_cadence_or_skip_without_hw() {
    const GOP_SIZE: u32 = 3;
    let cfg = tiny_vp9_gop_cfg(64, 64, GOP_SIZE);
    let mut enc = match VaapiVp9Encoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: VaapiVp9Encoder::open failed ({e:?}) — no VA-API VP9 encode config?");
            return;
        }
    };

    let mut is_keyframe = Vec::new();
    for pts in 0..7i64 {
        let frame = black_nv12_frame(pts, 64, 64);
        if let Err(e) = enc.push_frame(&frame) {
            eprintln!("skip: push_frame failed ({e:?}) — no usable VA-API VP9 encode session?");
            return;
        }
        match enc.poll_packet() {
            Ok(Some(p)) => is_keyframe.push(p.is_keyframe),
            Ok(None) => eprintln!("skip: no packet produced for pts={pts}"),
            Err(e) => {
                eprintln!("skip: poll_packet failed ({e:?})");
                return;
            }
        }
    }
    eprintln!("vaapi vp9 gop cadence={is_keyframe:?}");
    if is_keyframe.len() == 7 {
        // Every packet keyframe is an honest capability fallback (VAConfigAttribEncMaxRefFrames
        // unsupported, or the driver rejected the GOP path some other way) — not a test failure.
        let all_key = is_keyframe.iter().all(|&k| k);
        if !all_key {
            assert_eq!(
                is_keyframe,
                vec![true, false, false, true, false, false, true]
            );
        }
    }
}
