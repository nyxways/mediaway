#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::super::hevc_slice::{HevcSliceType, ShortTermRefPicSet};
use super::*;
use mediaway_common::{CodecKind, Rational};

fn test_sps() -> HevcSps {
    HevcSps {
        general_profile_idc: 1,
        pic_width_in_luma_samples: 64,
        pic_height_in_luma_samples: 64,
        log2_max_pic_order_cnt_lsb: 8,
        max_dec_pic_buffering: 2,
        log2_min_cb_size: 3,
        log2_diff_max_min_cb_size: 2,
        log2_min_tb_size: 2,
        log2_diff_max_min_tb_size: 3,
        max_transform_hierarchy_depth_inter: 3,
        max_transform_hierarchy_depth_intra: 3,
        amp_enabled_flag: true,
        sample_adaptive_offset_enabled_flag: false,
        sps_temporal_mvp_enabled_flag: false,
        strong_intra_smoothing_enabled_flag: true,
    }
}

fn test_pps() -> HevcPps {
    HevcPps {
        pps_pic_parameter_set_id: 0,
        dependent_slice_segments_enabled_flag: false,
        output_flag_present_flag: false,
        num_extra_slice_header_bits: 0,
        sign_data_hiding_enabled_flag: false,
        cabac_init_present_flag: false,
        num_ref_idx_l0_default_active: 1,
        num_ref_idx_l1_default_active: 1,
        init_qp: 26,
        constrained_intra_pred_flag: false,
        transform_skip_enabled_flag: false,
        cu_qp_delta_enabled_flag: false,
        diff_cu_qp_delta_depth: 0,
        pps_cb_qp_offset: 0,
        pps_cr_qp_offset: 0,
        pps_slice_chroma_qp_offsets_present_flag: false,
        weighted_pred_flag: false,
        weighted_bipred_flag: false,
        transquant_bypass_enabled_flag: false,
        pps_loop_filter_across_slices_enabled_flag: false,
        lists_modification_present_flag: false,
        log2_parallel_merge_level_minus2: 0,
    }
}

fn test_idr_header() -> HevcSliceSegmentHeader {
    HevcSliceSegmentHeader {
        slice_type: HevcSliceType::I,
        slice_pic_parameter_set_id: 0,
        pic_order_cnt_lsb: None,
        short_term_rps: None,
        slice_sao_luma_flag: false,
        slice_sao_chroma_flag: false,
        slice_temporal_mvp_enabled_flag: false,
        num_ref_idx_l0_active: 0,
        cabac_init_flag: false,
        five_minus_max_num_merge_cand: 0,
        slice_qp_delta: 0,
        slice_cb_qp_offset: 0,
        slice_cr_qp_offset: 0,
        slice_loop_filter_across_slices_enabled_flag: false,
        st_rps_bits: 0,
        bits_consumed: 16,
    }
}

fn test_p_header() -> HevcSliceSegmentHeader {
    HevcSliceSegmentHeader {
        slice_type: HevcSliceType::P,
        pic_order_cnt_lsb: Some(1),
        short_term_rps: Some(ShortTermRefPicSet::default()),
        num_ref_idx_l0_active: 1,
        five_minus_max_num_merge_cand: 4,
        st_rps_bits: 6,
        ..test_idr_header()
    }
}

#[test]
fn round_up_8_rounds_to_next_multiple() {
    assert_eq!(round_up_8(0), 0);
    assert_eq!(round_up_8(1), 8);
    assert_eq!(round_up_8(8), 8);
    assert_eq!(round_up_8(9), 16);
    assert_eq!(round_up_8(64), 64);
}

#[test]
fn validate_accepts_hevc_nv12_config() {
    let cfg = VideoDecoderConfig::hevc(64, 64, Rational::new(1, 30));
    assert!(validate(&cfg).is_ok());
}

#[test]
fn validate_rejects_non_hevc_codec() {
    let mut cfg = VideoDecoderConfig::hevc(64, 64, Rational::new(1, 30));
    cfg.codec = CodecKind::H264;
    assert_eq!(validate(&cfg), Err(DecodeError::Unsupported));
}

#[test]
fn validate_rejects_non_nv12_pixel_format() {
    let mut cfg = VideoDecoderConfig::hevc(64, 64, Rational::new(1, 30));
    cfg.pixel_format = PixelFormat::I420;
    assert_eq!(validate(&cfg), Err(DecodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_time_base_denominator() {
    let mut cfg = VideoDecoderConfig::hevc(64, 64, Rational::new(1, 30));
    cfg.time_base = Rational::new(1, 0);
    assert_eq!(validate(&cfg), Err(DecodeError::InvalidInput));
}

#[test]
fn build_pic_param_succeeds_for_idr() {
    let sps = test_sps();
    let pps = test_pps();
    let header = test_idr_header();
    let result = build_pic_param(&sps, &pps, true, &header, 1, 0, None);
    assert!(result.is_ok());
}

#[test]
fn build_pic_param_succeeds_with_reference() {
    let sps = test_sps();
    let pps = test_pps();
    let header = test_p_header();
    let result = build_pic_param(&sps, &pps, false, &header, 1, 4, Some((2, 3)));
    assert!(result.is_ok());
}

#[test]
fn build_slice_param_succeeds_for_idr() {
    let header = test_idr_header();
    let result = build_slice_param(&header, 42, None);
    assert!(result.is_ok());
}

#[test]
fn build_slice_param_succeeds_with_p_reference() {
    let header = test_p_header();
    let result = build_slice_param(&header, 42, Some((2, 3)));
    assert!(result.is_ok());
}

#[test]
fn build_slice_param_rejects_nal_length_overflowing_u32() {
    let header = test_idr_header();
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

/// Attempts to open a real VA-API display and HEVC CPU-output decode session.
///
/// **Expected to skip in this development session** — see
/// [`adr/linux/0003-vaapi-hevc-p-slice-dpb.md`](../../adr/linux/0003-vaapi-hevc-p-slice-dpb.md)'s
/// "zero real-hardware verification" caveat: this box has no working `/dev/dri/renderD*` VA-API
/// device (WSL2 here has broken VA-API / software-only Vulkan; no real Linux GPU is exposed).
#[test]
fn open_vaapi_hevc_cpu_or_skip() {
    let cfg = VideoDecoderConfig::hevc(64, 64, Rational::new(1, 30));
    let mut dec = match VaapiHevcDecoder::open(&cfg) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skip: no VA-API display available ({e:?})");
            return;
        }
    };
    dec.flush().expect("flush without packets");
    assert!(dec.poll_frame().expect("poll").is_none());
}
