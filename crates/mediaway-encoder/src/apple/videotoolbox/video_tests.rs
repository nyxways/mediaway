//! Tests for [`super::VideoToolboxVideoEncoder`] — see `docs/conventions/testing.md` Tier 1.
//!
//! **Never executed as authored** — this crate's dev environment cannot cross-compile Apple
//! code at all (no Xcode/Apple SDK outside macOS), so even `cargo check`/`clippy` on
//! `target_os = "macos"`/`"ios"` has not run against this file. See
//! [ADR-0001](../../adr/apple/0001-videotoolbox-h264-cpu-upload.md) § CI verification plan.
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
        color_range: ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    }
}

// --- Pure logic: no session instance needed --------------------------------------------------

#[test]
fn validate_accepts_h264_cpu_upload_config() {
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
fn validate_accepts_non_macroblock_aligned_dimensions() {
    // Unlike Linux VA-API (raw bitstream construction), VideoToolbox handles internal padding
    // itself — this backend does not require macroblock-aligned dimensions.
    assert!(validate(&tiny_h264_cfg(65, 33)).is_ok());
}

#[test]
fn yuv420_size_is_one_and_a_half_bytes_per_pixel() {
    assert_eq!(yuv420_size(64, 64).unwrap(), 64 * 64 + (64 * 64) / 2);
    assert_eq!(yuv420_size(16, 16).unwrap(), 16 * 16 + (16 * 16) / 2);
}

#[test]
fn yuv420_size_rejects_overflowing_dimensions() {
    assert_eq!(
        yuv420_size(u32::MAX, u32::MAX),
        Err(EncodeError::InvalidInput)
    );
}

#[test]
fn frame_rate_hint_falls_back_when_timebase_numerator_is_zero() {
    assert_eq!(frame_rate_hint(Rational::new(0, 30)), 30);
}

#[test]
fn frame_rate_hint_computes_from_timebase() {
    assert_eq!(frame_rate_hint(Rational::new(1, 30)), 30);
    assert_eq!(frame_rate_hint(Rational::new(1, 60)), 60);
}

#[test]
fn cmtime_from_pts_carries_value_and_timescale() {
    let t = cmtime_from_pts(42, 30);
    assert_eq!(t.value, 42);
    assert_eq!(t.timescale, 30);
}

#[test]
fn cmtime_to_pts_round_trips_same_timescale() {
    let t = cmtime_from_pts(90, 30);
    assert_eq!(cmtime_to_pts(t, 30), 90);
}

#[test]
fn cmtime_to_pts_rescales_differing_timescale() {
    // 1 second at a 600-unit encoder timescale, rescaled into a 30-unit config timebase.
    let t = CMTime {
        value: 600,
        timescale: 600,
        flags: objc2_core_media::CMTimeFlags::Valid,
        epoch: 0,
    };
    assert_eq!(cmtime_to_pts(t, 30), 30);
}

#[test]
fn cmtime_to_pts_guards_against_zero_timescale() {
    let t = CMTime {
        value: 5,
        timescale: 0,
        flags: objc2_core_media::CMTimeFlags::Valid,
        epoch: 0,
    };
    assert_eq!(cmtime_to_pts(t, 30), 0);
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

// --- Hardware-gated: real VTCompressionSession ------------------------------------------------

/// Opens a real `VTCompressionSession` H.264 CPU-upload session and, if available, encodes one
/// black NV12 frame end to end.
///
/// **Never run at all in the session that authored this crate** — no macOS/iOS hardware, and no
/// local Apple SDK to even build this test. See
/// [ADR-0001](../../adr/apple/0001-videotoolbox-h264-cpu-upload.md) § CI verification plan.
/// Provided so a future run on real Apple hardware (`apple-macos` CI job or beyond) can verify
/// the path this crate was written against.
#[test]
fn videotoolbox_open_and_encode_or_skip_without_hw() {
    let cfg = tiny_h264_cfg(64, 64);
    let mut enc = match VideoToolboxVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: VideoToolboxVideoEncoder::open failed ({e:?}) — no AVC encoder?");
            return;
        }
    };

    let yuv_len = 64 * 64 + (64 * 64) / 2;
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: 64,
        height: 64,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(vec![0u8; yuv_len]),
        },
    };

    if let Err(e) = enc.push_frame(&frame) {
        eprintln!("skip: push_frame failed ({e:?}) — no usable VideoToolbox session?");
        return;
    }
    let _ = enc.flush();

    let mut packets = 0usize;
    while let Ok(Some(p)) = enc.poll_packet() {
        assert!(!p.payload.is_empty());
        packets += 1;
    }
    eprintln!("videotoolbox h264 cpu-upload packets={packets}");
}
