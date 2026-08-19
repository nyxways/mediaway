#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use mediaway_common::Rational;

#[test]
fn round_up_16_rounds_to_next_multiple() {
    assert_eq!(round_up_16(0), 0);
    assert_eq!(round_up_16(1), 16);
    assert_eq!(round_up_16(16), 16);
    assert_eq!(round_up_16(17), 32);
}

#[test]
fn validate_accepts_av1_nv12_config() {
    let mut cfg = VideoDecoderConfig::av1(64, 64, Rational::new(1, 30));
    cfg.output = VideoOutputPreference::CpuFramesOk;
    assert!(validate(&cfg).is_ok());
}

#[test]
fn validate_rejects_non_av1_codec() {
    let mut cfg = VideoDecoderConfig::av1(64, 64, Rational::new(1, 30));
    cfg.codec = mediaway_common::CodecKind::H264;
    assert_eq!(validate(&cfg), Err(DecodeError::Unsupported));
}

#[test]
fn validate_rejects_non_nv12_pixel_format() {
    let mut cfg = VideoDecoderConfig::av1(64, 64, Rational::new(1, 30));
    cfg.pixel_format = PixelFormat::I420;
    assert_eq!(validate(&cfg), Err(DecodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_time_base_denominator() {
    let mut cfg = VideoDecoderConfig::av1(64, 64, Rational::new(1, 30));
    cfg.time_base = Rational::new(1, 0);
    assert_eq!(validate(&cfg), Err(DecodeError::InvalidInput));
}

#[test]
fn build_slice_param_succeeds_for_small_tile() {
    let result = build_slice_param(&[0xAA, 0xBB, 0xCC]);
    assert!(result.is_ok());
}

#[test]
fn seed_sequence_header_returns_none_for_empty_extra_data() {
    assert!(seed_sequence_header(&Bytes::new()).is_none());
}

#[test]
fn seed_sequence_header_returns_none_for_malformed_extra_data() {
    // Not a valid OBU stream at all (forbidden bit set) -> split_obus errors, treated as
    // best-effort absence rather than propagated (see this crate's H.264 seed_params parity).
    assert!(seed_sequence_header(&Bytes::from(vec![0xFF])).is_none());
}
