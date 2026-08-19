#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use mediaway_common::Rational;

fn test_key_header() -> Header {
    Header {
        is_key: true,
        error_resilient_mode: false,
        refresh_frame_flags: 0xff,
        ref_frame_idx: [0, 0, 0],
        ref_frame_sign_bias: [false, false, false],
        width: 64,
        height: 64,
        allow_high_precision_mv: false,
        interpolation_filter: 0,
        refresh_frame_context: false,
        frame_parallel_decoding_mode: true,
        frame_context_idx: 0,
        reset_frame_context: 0,
        loop_filter: loop_filter::LoopFilterParams {
            level: 0,
            sharpness: 0,
        },
        quantization: quantization::QuantizationParams {
            base_q_idx: 10,
            lossless: false,
        },
        first_partition_size: 5,
        frame_header_length_in_bytes: 14,
    }
}

#[test]
fn round_up_8_rounds_to_next_multiple() {
    assert_eq!(round_up_8(0), 0);
    assert_eq!(round_up_8(1), 8);
    assert_eq!(round_up_8(8), 8);
    assert_eq!(round_up_8(9), 16);
}

#[test]
fn validate_accepts_vp9_nv12_config() {
    let mut cfg = VideoDecoderConfig::vp9(64, 64, Rational::new(1, 30));
    cfg.output = VideoOutputPreference::CpuFramesOk;
    assert!(validate(&cfg).is_ok());
}

#[test]
fn validate_rejects_non_vp9_codec() {
    let mut cfg = VideoDecoderConfig::vp9(64, 64, Rational::new(1, 30));
    cfg.codec = mediaway_common::CodecKind::H264;
    assert_eq!(validate(&cfg), Err(DecodeError::Unsupported));
}

#[test]
fn validate_rejects_non_nv12_pixel_format() {
    let mut cfg = VideoDecoderConfig::vp9(64, 64, Rational::new(1, 30));
    cfg.pixel_format = PixelFormat::I420;
    assert_eq!(validate(&cfg), Err(DecodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_time_base_denominator() {
    let mut cfg = VideoDecoderConfig::vp9(64, 64, Rational::new(1, 30));
    cfg.time_base = Rational::new(1, 0);
    assert_eq!(validate(&cfg), Err(DecodeError::InvalidInput));
}

#[test]
fn build_pic_param_maps_dimensions_profile_and_bit_depth() {
    let header = test_key_header();
    let refs = [VA_INVALID_ID; VP9_REF_SLOTS];
    let pic_param = build_pic_param(&header, refs).unwrap();
    let inner = pic_param.inner();
    assert_eq!(inner.frame_width, 64);
    assert_eq!(inner.frame_height, 64);
    assert_eq!(inner.profile, 0);
    assert_eq!(inner.bit_depth, 8);
    assert_eq!(inner.frame_header_length_in_bytes, 14);
    assert_eq!(inner.first_partition_size, 5);
    assert_eq!(inner.log2_tile_rows, 0);
    assert_eq!(inner.log2_tile_columns, 0);
    assert_eq!(inner.reference_frames, [VA_INVALID_ID; VP9_REF_SLOTS]);
}

#[test]
fn build_pic_param_rejects_width_overflowing_u16() {
    let mut header = test_key_header();
    header.width = u32::from(u16::MAX) + 1;
    let refs = [VA_INVALID_ID; VP9_REF_SLOTS];
    assert!(build_pic_param(&header, refs).is_err());
}

#[test]
fn build_slice_param_offsets_past_the_uncompressed_header() {
    let header = test_key_header(); // frame_header_length_in_bytes = 14
    let total_len = 14 + 100;
    let result = build_slice_param(&header, total_len);
    assert!(result.is_ok());
}

#[test]
fn build_slice_param_errors_when_payload_shorter_than_header_length() {
    let header = test_key_header(); // frame_header_length_in_bytes = 14
    let result = build_slice_param(&header, 5); // shorter than the uncompressed header itself
    assert_eq!(result.err(), Some(DecodeError::InvalidInput));
}
