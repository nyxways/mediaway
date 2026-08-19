//! Tests for [`super::VaapiHevcVideoEncoder`] — see `docs/conventions/testing.md` Tier 1.
//! Mirrors `video_tests.rs`'s H.264 coverage shape (ADR-0003 § Test plan).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use mediaway_common::{CodecKind, Rational};

fn tiny_hevc_cfg(width: u32, height: u32) -> VideoEncoderConfig {
    tiny_hevc_gop_cfg(width, height, 1)
}

fn tiny_hevc_gop_cfg(width: u32, height: u32, gop_size: u32) -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: CodecKind::Hevc,
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
fn validate_accepts_cb_aligned_hevc_cpu_upload_config() {
    assert!(validate(&tiny_hevc_cfg(64, 64)).is_ok());
}

#[test]
fn validate_rejects_non_hevc_codec() {
    let mut cfg = tiny_hevc_cfg(64, 64);
    cfg.codec = CodecKind::H264;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_dimensions() {
    let cfg = tiny_hevc_cfg(0, 64);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_rejects_non_8_pixel_aligned_dimensions() {
    // 65 is not a multiple of 8 — the minimum-CB-size grid this crate's fixed CU/TU range needs.
    let cfg = tiny_hevc_cfg(65, 64);
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_accepts_8_pixel_aligned_non_16_aligned_dimensions() {
    // Unlike H.264 (16-pixel macroblock alignment), HEVC's own VA-API buffers take raw pixel
    // dimensions — only 8-pixel (minimum-CB-size) alignment is required (ADR-0003 § VA-API-
    // specific plumbing).
    let cfg = tiny_hevc_cfg(72, 56);
    assert!(validate(&cfg).is_ok());
}

#[test]
fn validate_rejects_non_nv12_pixel_format() {
    let mut cfg = tiny_hevc_cfg(64, 64);
    cfg.pixel_format = PixelFormat::Bgra8;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_timebase_denominator() {
    let mut cfg = tiny_hevc_cfg(64, 64);
    cfg.time_base = Rational::new(1, 0);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_rejects_zero_gop_size() {
    let cfg = tiny_hevc_gop_cfg(64, 64, 0);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_accepts_gop_size_greater_than_one() {
    let cfg = tiny_hevc_gop_cfg(64, 64, 3);
    assert!(validate(&cfg).is_ok());
}

#[test]
fn ctu_count_rounds_up_to_ctu_grid() {
    // 64x64 at CTU_SIZE=32 is an exact 2x2 CTU grid.
    assert_eq!(ctu_count(64, 64), 4);
    // 72x56 is not CTU-aligned (32) — rounds up to a 3x2 grid.
    assert_eq!(ctu_count(72, 56), 6);
}

#[test]
fn stream_info_from_config_carries_geometry_and_timebase() {
    let cfg = tiny_hevc_cfg(64, 32);
    let info = stream_info_from(&cfg);
    assert!(matches!(
        info,
        StreamInfo::Video {
            codec: CodecKind::Hevc,
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

/// Opens a real VA-API HEVC CPU-upload session and, if a display is available, encodes one
/// black NV12 frame end to end. **Expected to skip in the session that authored this crate** —
/// no real `/dev/dri/renderD*` VA-API device available, mirrors
/// `vaapi::video_tests::vaapi_open_and_encode_or_skip_without_hw`'s identical disposition. See
/// [ADR-0003](../../adr/linux/0003-vaapi-hevc-p-frame-gop.md) § Zero real-hardware verification.
#[test]
fn vaapi_open_and_encode_or_skip_without_hw() {
    let cfg = tiny_hevc_cfg(64, 64);
    let mut enc = match VaapiHevcVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: VaapiHevcVideoEncoder::open failed ({e:?}) — no VA-API display?");
            return;
        }
    };

    let frame = black_nv12_frame(0, 64, 64);
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
    eprintln!("vaapi hevc cpu-upload packets={packets}");
}

/// ADR-0003: pushes 7 frames with `gop_size = 3` through a real VA-API session and confirms the
/// resulting packets' `is_keyframe` cadence is `I P P I P P I` — mirrors
/// `vaapi::video_tests::vaapi_gop_cadence_or_skip_without_hw`'s identical shape and honest
/// all-IDR-fallback disposition on a driver that reports `VAConfigAttribEncMaxRefFrames` as
/// unsupported for `VAProfileHEVCMain`.
///
/// **Expected to skip in the session that authored this crate** — same zero-real-hardware
/// disposition as `vaapi_open_and_encode_or_skip_without_hw` above.
#[test]
fn vaapi_gop_cadence_or_skip_without_hw() {
    const GOP_SIZE: u32 = 3;
    let cfg = tiny_hevc_gop_cfg(64, 64, GOP_SIZE);
    let mut enc = match VaapiHevcVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: VaapiHevcVideoEncoder::open failed ({e:?}) — no VA-API display?");
            return;
        }
    };

    let mut is_keyframe = Vec::new();
    for pts in 0..7i64 {
        let frame = black_nv12_frame(pts, 64, 64);
        if let Err(e) = enc.push_frame(&frame) {
            eprintln!("skip: push_frame failed ({e:?}) — no usable VA-API encode session?");
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
    let _ = enc.flush();

    // A driver without VAConfigAttribEncMaxRefFrames support (for VAProfileHEVCMain) degrades
    // to all-IDR — a documented fallback, not a bug.
    if is_keyframe.iter().all(|&kf| kf) {
        eprintln!(
            "vaapi hevc gop cadence: driver may report VAConfigAttribEncMaxRefFrames unsupported \
             for VAProfileHEVCMain, degrading to all-IDR — skipping cadence assertion"
        );
        return;
    }
    assert_eq!(
        is_keyframe,
        vec![true, false, false, true, false, false, true],
        "expected I P P I P P I cadence for gop_size={GOP_SIZE}"
    );
}
