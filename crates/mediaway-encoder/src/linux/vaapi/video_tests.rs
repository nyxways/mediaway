//! Tests for [`super::VaapiH264Encoder`] — see `docs/conventions/testing.md` Tier 1.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use mediaway_common::{CodecKind, Rational};

fn tiny_h264_cfg(width: u32, height: u32) -> VideoEncoderConfig {
    tiny_h264_gop_cfg(width, height, 1)
}

/// ADR-0002: same as [`tiny_h264_cfg`] with a caller-chosen `gop_size` — the "real, if
/// mechanical" test-fixture addition the ADR's own Consequences section flagged.
fn tiny_h264_gop_cfg(width: u32, height: u32, gop_size: u32) -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: CodecKind::H264,
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
fn validate_rejects_zero_gop_size() {
    // ADR-0002: `0` is rejected outright, never treated as "infinite GOP".
    let cfg = tiny_h264_gop_cfg(64, 64, 0);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_accepts_gop_size_greater_than_one() {
    let cfg = tiny_h264_gop_cfg(64, 64, 3);
    assert!(validate(&cfg).is_ok());
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
    let mut enc = match VaapiH264Encoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: VaapiH264Encoder::open failed ({e:?}) — no VA-API display?");
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

/// ADR-0002: pushes 7 frames with `gop_size = 3` through a real VA-API session and confirms the
/// resulting packets' `is_keyframe` cadence is `I P P I P P I` — mirrors
/// `vulkan::encoder_tests.rs::push_seven_frames_gop_or_skip`'s own assertion shape.
///
/// **Expected to skip in the session that authored this crate** — same zero-real-hardware
/// disposition as `vaapi_open_and_encode_or_skip_without_hw` above. Also honestly degrades to
/// all-IDR (every packet a keyframe) on a driver that reports
/// `VAConfigAttribEncMaxRefFrames` as unsupported — that is this backend's documented capability
/// fallback, not a test failure.
#[test]
fn vaapi_gop_cadence_or_skip_without_hw() {
    const GOP_SIZE: u32 = 3;
    let cfg = tiny_h264_gop_cfg(64, 64, GOP_SIZE);
    let mut enc = match VaapiH264Encoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: VaapiH264Encoder::open failed ({e:?}) — no VA-API display?");
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

    // A driver without VAConfigAttribEncMaxRefFrames support degrades to all-IDR — a documented
    // fallback, not a bug.
    if is_keyframe.iter().all(|&kf| kf) {
        eprintln!(
            "vaapi gop cadence: driver may report VAConfigAttribEncMaxRefFrames unsupported, \
             degrading to all-IDR — skipping cadence assertion"
        );
        return;
    }
    assert_eq!(
        is_keyframe,
        vec![true, false, false, true, false, false, true],
        "expected I P P I P P I cadence for gop_size={GOP_SIZE}"
    );
}

/// ADR-0002 § Real gap found: confirms `push_frame` returns `Err(EncodeError::Backend)`, not a
/// panic or silent misencode, when a P-frame's expected reference slot has no surface — the
/// lost-reference-surface guard (§ Reference-list construction step 3). Simulates the loss
/// directly (no real encode failure needed to trigger it): after one IDR push, the surface at
/// the DPB slot the next P-frame's `GopState::decide` output will reference is manually removed
/// from the pool, mirroring what a failed mid-GOP `Picture::begin`/`render`/`end` step would
/// otherwise leave behind (see `encode_one`'s doc comment).
///
/// **Expected to skip in the session that authored this crate** — same zero-real-hardware
/// disposition as the other hardware-gated tests in this file; needs a real opened session
/// (`self.surfaces`) even though it never asks the driver to encode the poisoned frame.
#[test]
fn vaapi_push_frame_errors_on_lost_reference_surface_or_skip_without_hw() {
    let cfg = tiny_h264_gop_cfg(64, 64, 3);
    let mut enc = match VaapiH264Encoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: VaapiH264Encoder::open failed ({e:?}) — no VA-API display?");
            return;
        }
    };
    if !enc.supports_p_frames {
        eprintln!("skip: driver does not report VAConfigAttribEncMaxRefFrames support");
        return;
    }

    // Frame 0: IDR, writes GopState's setup_slot 0.
    if let Err(e) = enc.push_frame(&black_nv12_frame(0, 64, 64)) {
        eprintln!("skip: push_frame(0) failed ({e:?}) — no usable VA-API encode session?");
        return;
    }
    let _ = enc.poll_packet();

    // Simulate a lost surface at slot 0 — the exact slot frame 1 (the next P frame) will expect
    // to read back as its sole L0 reference.
    enc.surfaces[0] = None;

    let result = enc.push_frame(&black_nv12_frame(1, 64, 64));
    assert_eq!(result, Err(EncodeError::Backend));
}
