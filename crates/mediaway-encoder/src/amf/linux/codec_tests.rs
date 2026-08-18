//! Tests for [`super`]'s pure `VideoEncoderConfig` → AMF field conversions — no
//! `shiguredo_amf`/AMD driver needed (see `docs/conventions/testing.md` Tier 1).
#![allow(clippy::unwrap_used, reason = "test module may unwrap")]

use super::*;

#[test]
fn is_supported_video_codec_accepts_only_h264() {
    assert!(is_supported_video_codec(CodecKind::H264));
    assert!(!is_supported_video_codec(CodecKind::Hevc));
    assert!(!is_supported_video_codec(CodecKind::Av1));
    assert!(!is_supported_video_codec(CodecKind::Vp9));
}

#[test]
fn framerate_from_time_base_is_the_reciprocal() {
    // 1/30 seconds-per-tick timebase -> 30/1 fps.
    assert_eq!(framerate_from_time_base(Rational::new(1, 30)), (30, 1));
    // 1001/30000 (NTSC-style 29.97fps) seconds-per-tick -> 30000/1001 fps.
    assert_eq!(
        framerate_from_time_base(Rational::new(1001, 30_000)),
        (30_000, 1001)
    );
}

#[test]
fn framerate_from_time_base_never_returns_zero_denominator() {
    // A degenerate `num == 0` timebase must not produce a `framerate_den == 0`
    // (division by zero downstream in `shiguredo_amf`).
    let (_, den) = framerate_from_time_base(Rational::new(0, 30));
    assert_eq!(den, 1);
}

#[test]
fn bps_to_kbps_truncates_toward_zero() {
    assert_eq!(bps_to_kbps(0), 0);
    assert_eq!(bps_to_kbps(500_000), 500);
    assert_eq!(bps_to_kbps(1_999), 1);
}

#[test]
fn vbv_bytes_to_max_kbps_converts_bytes_to_kilobits() {
    assert_eq!(vbv_bytes_to_max_kbps(0), 0);
    // 125_000 bytes = 1_000_000 bits = 1_000 kbit.
    assert_eq!(vbv_bytes_to_max_kbps(125_000), 1_000);
}

#[test]
fn nv12_size_is_one_and_a_half_bytes_per_pixel() {
    assert_eq!(nv12_size(64, 64).unwrap(), 64 * 64 + (64 * 64) / 2);
    assert_eq!(nv12_size(0, 0).unwrap(), 0);
}
