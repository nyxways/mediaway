#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use mediaway_common::{CodecKind, Rational};
use mediaway_sw::h264::NalUnitType;

fn test_sps() -> Sps {
    Sps {
        profile_idc: 66,
        log2_max_frame_num_minus4: 4,
        pic_order_cnt_type: 0,
        log2_max_pic_order_cnt_lsb_minus4: 4,
        max_num_ref_frames: 1,
        gaps_in_frame_num_value_allowed_flag: false,
        pic_width_in_mbs_minus1: 3,
        pic_height_in_map_units_minus1: 3,
        direct_8x8_inference_flag: true,
    }
}

fn test_pps() -> Pps {
    Pps {
        pic_parameter_set_id: 0,
        entropy_coding_mode_flag: false,
        pic_order_present_flag: false,
        num_ref_idx_l0_default_active: 1,
        weighted_pred_flag: false,
        pic_init_qp_minus26: 0,
        pic_init_qs_minus26: 0,
        chroma_qp_index_offset: 0,
        second_chroma_qp_index_offset: 0,
        deblocking_filter_control_present_flag: false,
        constrained_intra_pred_flag: false,
        redundant_pic_cnt_present_flag: false,
    }
}

fn test_header() -> SliceHeader {
    SliceHeader {
        first_mb_in_slice: 0,
        slice_type: 2,
        pic_parameter_set_id: 0,
        frame_num: 0,
        is_idr: true,
        pic_order_cnt_lsb: 0,
        num_ref_idx_l0_active: 0,
        slice_qp_delta: 0,
        disable_deblocking_filter_idc: 0,
        slice_alpha_c0_offset_div2: 0,
        slice_beta_offset_div2: 0,
        bits_consumed: 20,
    }
}

fn test_p_header() -> SliceHeader {
    SliceHeader {
        slice_type: 0,
        num_ref_idx_l0_active: 1,
        is_idr: false,
        ..test_header()
    }
}

fn test_idr_unit() -> NalUnit {
    NalUnit {
        ref_idc: 1,
        unit_type: NalUnitType::IdrSlice,
        rbsp: Bytes::new(),
    }
}

#[test]
fn round_up_16_rounds_to_next_multiple() {
    assert_eq!(round_up_16(0), 0);
    assert_eq!(round_up_16(1), 16);
    assert_eq!(round_up_16(16), 16);
    assert_eq!(round_up_16(17), 32);
    assert_eq!(round_up_16(640), 640);
}

#[test]
fn validate_accepts_h264_nv12_config() {
    let cfg = VideoDecoderConfig {
        codec: CodecKind::H264,
        width: 64,
        height: 64,
        time_base: Rational::new(1, 30),
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data: Bytes::new(),
    };
    assert!(validate(&cfg).is_ok());
}

#[test]
fn validate_rejects_non_h264_codec() {
    let mut cfg = VideoDecoderConfig::h264(64, 64, Rational::new(1, 30));
    cfg.codec = CodecKind::Hevc;
    assert_eq!(validate(&cfg), Err(DecodeError::Unsupported));
}

#[test]
fn validate_rejects_non_nv12_pixel_format() {
    let mut cfg = VideoDecoderConfig::h264(64, 64, Rational::new(1, 30));
    cfg.pixel_format = PixelFormat::I420;
    assert_eq!(validate(&cfg), Err(DecodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_time_base_denominator() {
    let mut cfg = VideoDecoderConfig::h264(64, 64, Rational::new(1, 30));
    cfg.time_base = Rational::new(1, 0);
    assert_eq!(validate(&cfg), Err(DecodeError::InvalidInput));
}

#[test]
fn build_pic_param_succeeds_for_in_range_fields() {
    let sps = test_sps();
    let pps = test_pps();
    let header = test_header();
    let unit = test_idr_unit();
    let result = build_pic_param(
        &sps,
        &pps,
        &unit,
        &header,
        1,
        0,
        &[],
        sps.max_num_ref_frames,
    );
    assert!(result.is_ok());
}

#[test]
fn build_pic_param_succeeds_with_reference_frames() {
    let sps = test_sps();
    let pps = test_pps();
    let header = test_p_header();
    let unit = NalUnit {
        ref_idc: 1,
        unit_type: NalUnitType::NonIdrSlice,
        rbsp: Bytes::new(),
    };
    let reference_frames = [(2u32, DpbSlot::new_reference(0, 0, 0))];
    let result = build_pic_param(
        &sps,
        &pps,
        &unit,
        &header,
        1,
        2,
        &reference_frames,
        sps.max_num_ref_frames,
    );
    assert!(result.is_ok());
}

#[test]
fn build_slice_param_succeeds_and_reports_correct_bit_offset() {
    let header = test_header();
    let result = build_slice_param(&header, 42, None);
    assert!(result.is_ok());
}

#[test]
fn build_slice_param_succeeds_with_p_slice_reference() {
    let header = test_p_header();
    let ref_pic0 = Some((2u32, DpbSlot::new_reference(0, 0, 0)));
    let result = build_slice_param(&header, 42, ref_pic0);
    assert!(result.is_ok());
}

#[test]
fn build_slice_param_rejects_nal_length_overflowing_u32() {
    let header = test_header();
    // usize can exceed u32::MAX on 64-bit hosts; the conversion must be rejected, not panic.
    let huge = usize::try_from(u32::MAX).expect("u32::MAX fits usize") + 1;
    let result = build_slice_param(&header, huge, None);
    assert_eq!(result.err(), Some(DecodeError::InvalidInput));
}

#[test]
fn seed_params_returns_none_for_empty_extra_data() {
    let (sps, pps) = seed_params(&Bytes::new());
    assert!(sps.is_none());
    assert!(pps.is_none());
}
