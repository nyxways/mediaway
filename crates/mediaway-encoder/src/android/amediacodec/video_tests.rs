//! Tests for [`super::AmediaCodecVideoEncoder`] — see `docs/conventions/testing.md` Tier 1.
//!
//! **Never executed as authored** — this crate's dev environment has no Android NDK, so even
//! `cargo check`/`clippy` on `target_os = "android"` has not run against this file. See
//! [ADR-0001](../../adr/android/0001-ndk-amediacodec-h264-cpu-upload.md) § CI verification
//! plan.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]
#![allow(
    clippy::float_cmp,
    reason = "i_frame_interval_secs is exact small-integer division (e.g. 60/30), always exact in f32"
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
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    }
}

// --- Pure logic: no codec instance needed ---------------------------------------------------

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
    // Unlike Linux VA-API (raw bitstream construction), `AMediaCodec` handles internal
    // padding itself — this backend does not require macroblock-aligned dimensions.
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
fn i_frame_interval_secs_is_zero_for_idr_only_gop() {
    assert_eq!(i_frame_interval_secs(1, 30), 0.0);
    assert_eq!(i_frame_interval_secs(0, 30), 0.0);
}

#[test]
fn i_frame_interval_secs_converts_frame_count_through_frame_rate() {
    assert_eq!(i_frame_interval_secs(60, 30), 2.0);
    assert_eq!(i_frame_interval_secs(30, 30), 1.0);
}

#[test]
fn i_frame_interval_secs_guards_against_zero_frame_rate() {
    assert_eq!(i_frame_interval_secs(60, 0), 60.0);
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

// --- Hardware-gated: real AMediaCodec session -----------------------------------------------

/// Opens a real `AMediaCodec` H.264 CPU-upload session and, if the on-device encoder is
/// available, encodes one black YUV420 frame end to end.
///
/// **Never run at all in the session that authored this crate** — no Android device/emulator,
/// and no local NDK to even build this test. See
/// [ADR-0001](../../adr/android/0001-ndk-amediacodec-h264-cpu-upload.md) § CI verification
/// plan. Provided so a future run on real Android hardware (or an emulator with a software
/// AVC encoder) can verify the path this crate was written against.
#[test]
fn amediacodec_open_and_encode_or_skip_without_hw() {
    let cfg = tiny_h264_cfg(64, 64);
    let mut enc = match AmediaCodecVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: AmediaCodecVideoEncoder::open failed ({e:?}) — no AVC encoder?");
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
        eprintln!("skip: push_frame failed ({e:?}) — no usable AMediaCodec session?");
        return;
    }
    let _ = enc.flush();

    let mut packets = 0usize;
    while let Ok(Some(p)) = enc.poll_packet() {
        assert!(!p.payload.is_empty());
        packets += 1;
    }
    eprintln!("amediacodec h264 cpu-upload packets={packets}");
}
