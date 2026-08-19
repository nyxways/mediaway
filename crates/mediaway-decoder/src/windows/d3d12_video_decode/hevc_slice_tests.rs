//! Pure unit tests for [`super::parse_slice_header`]/[`super::ShortTermRefPicSet`] against
//! hand-built RBSP bitstreams (same tiny Exp-Golomb bit writer approach as
//! `h264_slice_tests.rs`, test-only, duplicated rather than shared).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use smallvec::SmallVec;

use super::{ShortTermRefPicEntry, ShortTermRefPicSet, SliceType, parse_slice_header};
use crate::windows::d3d12_video_decode::hevc_vps_sps_pps::{HevcNalUnitType, Pps, Sps};

struct BitWriter {
    bits: Vec<bool>,
}

impl BitWriter {
    fn new() -> Self {
        Self { bits: Vec::new() }
    }

    fn write_bit(&mut self, value: bool) {
        self.bits.push(value);
    }

    fn write_bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.write_bit((value >> i) & 1 == 1);
        }
    }

    fn write_ue(&mut self, value: u32) {
        let code = value + 1;
        let bit_len = 32 - code.leading_zeros();
        for _ in 0..bit_len - 1 {
            self.write_bit(false);
        }
        self.write_bits(code, bit_len);
    }

    fn finish(mut self) -> Vec<u8> {
        self.bits.push(true);
        while !self.bits.len().is_multiple_of(8) {
            self.bits.push(false);
        }
        let mut out = vec![0u8; self.bits.len() / 8];
        for (i, &bit) in self.bits.iter().enumerate() {
            if bit {
                out[i / 8] |= 1 << (7 - i % 8);
            }
        }
        out
    }
}

/// Simple, fixed fixture SPS/PPS — `sps_temporal_mvp_enabled_flag`/
/// `sample_adaptive_offset_enabled_flag` are both `false` and
/// `pps.output_flag_present_flag`/`num_extra_slice_header_bits` are both `0`, so slice
/// header fixtures below don't need to write those optional bit groups.
fn test_sps() -> Sps {
    Sps {
        pic_width_in_luma_samples: 352,
        pic_height_in_luma_samples: 288,
        log2_max_pic_order_cnt_lsb: 8,
        max_dec_pic_buffering: 4,
        log2_min_cb_size: 6,
        log2_diff_max_min_cb_size: 2,
        log2_min_tb_size: 2,
        log2_diff_max_min_tb_size: 3,
        max_transform_hierarchy_depth_inter: 2,
        max_transform_hierarchy_depth_intra: 1,
        amp_enabled_flag: true,
        sample_adaptive_offset_enabled_flag: false,
        sps_temporal_mvp_enabled_flag: false,
        strong_intra_smoothing_enabled_flag: true,
    }
}

fn test_pps() -> Pps {
    Pps {
        dependent_slice_segments_enabled_flag: false,
        output_flag_present_flag: false,
        num_extra_slice_header_bits: 0,
        sign_data_hiding_enabled_flag: false,
        cabac_init_present_flag: false,
        num_ref_idx_l0_default_active_minus1: 0,
        num_ref_idx_l1_default_active_minus1: 0,
        init_qp_minus26: 0,
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
        slice_segment_header_extension_present_flag: false,
    }
}

fn idr_i_slice_bytes() -> Vec<u8> {
    let mut w = BitWriter::new();
    w.write_bit(true); // first_slice_segment_in_pic_flag
    w.write_bit(false); // no_output_of_prior_pics_flag (IDR)
    w.write_ue(0); // slice_pic_parameter_set_id
    w.write_ue(2); // slice_type == I
    // no output_flag_present_flag/poc/rps/sao/temporal_mvp/num_ref_idx for I on IDR
    w.finish()
}

#[test]
fn parse_idr_i_slice_has_no_poc_or_rps() {
    let bytes = idr_i_slice_bytes();
    let sh = parse_slice_header(&bytes, HevcNalUnitType::Idr, &test_sps(), &test_pps())
        .expect("valid hand-built IDR I-slice header");
    assert_eq!(sh.slice_type, SliceType::I);
    assert!(sh.pic_order_cnt_lsb.is_none());
    assert!(sh.short_term_rps.is_none());
    assert_eq!(sh.num_ref_idx_l0_active_minus1, 0);
}

/// One-entry `short_term_ref_pic_set(0)` (single negative/`before` delta, used by the
/// current picture) — this module's single-forward-reference scope's own happy path.
fn write_single_ref_rps(w: &mut BitWriter, delta_poc_s0_minus1: u32) {
    w.write_ue(1); // num_negative_pics
    w.write_ue(0); // num_positive_pics
    w.write_ue(delta_poc_s0_minus1);
    w.write_bit(true); // used_by_curr_pic_s0_flag
}

fn trail_p_slice_bytes(poc_lsb: u32, num_ref_idx_active_override: Option<u32>) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.write_bit(true); // first_slice_segment_in_pic_flag
    w.write_ue(0); // slice_pic_parameter_set_id
    w.write_ue(1); // slice_type == P
    w.write_bits(poc_lsb, 8); // log2_max_pic_order_cnt_lsb == 8 in test_sps()
    w.write_bit(false); // short_term_ref_pic_set_sps_flag
    write_single_ref_rps(&mut w, 0); // delta_poc == -1
    // sps_temporal_mvp_enabled_flag / sample_adaptive_offset_enabled_flag are both
    // false in test_sps(), so neither bit group is written here.
    match num_ref_idx_active_override {
        None => w.write_bit(false), // num_ref_idx_active_override_flag
        Some(minus1) => {
            w.write_bit(true);
            w.write_ue(minus1);
        }
    }
    w.finish()
}

#[test]
fn parse_p_slice_single_forward_reference() {
    let bytes = trail_p_slice_bytes(5, None);
    let sh = parse_slice_header(&bytes, HevcNalUnitType::Trail(1), &test_sps(), &test_pps())
        .expect("valid hand-built single-ref P-slice header");
    assert_eq!(sh.slice_type, SliceType::P);
    assert_eq!(sh.pic_order_cnt_lsb, Some(5));
    let rps = sh.short_term_rps.expect("RPS present for non-IDR slice");
    assert_eq!(rps.s0.len(), 1);
    assert_eq!(rps.s0[0].delta_poc, -1);
    assert!(rps.s0[0].used_by_curr_pic);
    assert_eq!(rps.num_curr_pics(), 1);
    assert_eq!(sh.num_ref_idx_l0_active_minus1, 0);
}

#[test]
fn parse_p_slice_explicit_override_to_one_is_accepted() {
    let bytes = trail_p_slice_bytes(5, Some(0)); // num_ref_idx_l0_active_minus1 == 0
    let sh = parse_slice_header(&bytes, HevcNalUnitType::Trail(1), &test_sps(), &test_pps())
        .expect("num_ref_idx_l0_active == 1 via explicit override is in-scope");
    assert_eq!(sh.num_ref_idx_l0_active_minus1, 0);
}

#[test]
fn parse_p_slice_rejects_num_ref_idx_active_override_above_one() {
    let bytes = trail_p_slice_bytes(5, Some(1)); // num_ref_idx_l0_active_minus1 == 1 -> active == 2
    let result = parse_slice_header(&bytes, HevcNalUnitType::Trail(1), &test_sps(), &test_pps());
    assert!(result.is_err());
}

#[test]
fn parse_p_slice_rejects_multi_reference_rps() {
    let mut w = BitWriter::new();
    w.write_bit(true); // first_slice_segment_in_pic_flag
    w.write_ue(0); // slice_pic_parameter_set_id
    w.write_ue(1); // slice_type == P
    w.write_bits(5, 8); // poc_lsb
    w.write_bit(false); // short_term_ref_pic_set_sps_flag
    w.write_ue(2); // num_negative_pics
    w.write_ue(0); // num_positive_pics
    w.write_ue(0);
    w.write_bit(true); // first entry used_by_curr_pic
    w.write_ue(0);
    w.write_bit(true); // second entry ALSO used_by_curr_pic -> NumPicTotalCurr == 2
    let bytes = w.finish();
    let result = parse_slice_header(&bytes, HevcNalUnitType::Trail(1), &test_sps(), &test_pps());
    assert!(result.is_err());
}

#[test]
fn parse_rejects_multi_slice_picture() {
    let mut w = BitWriter::new();
    w.write_bit(false); // first_slice_segment_in_pic_flag == 0
    let bytes = w.finish();
    let result = parse_slice_header(&bytes, HevcNalUnitType::Trail(1), &test_sps(), &test_pps());
    assert!(result.is_err());
}

#[test]
fn parse_rejects_b_slice() {
    let mut w = BitWriter::new();
    w.write_bit(true); // first_slice_segment_in_pic_flag
    w.write_ue(0); // slice_pic_parameter_set_id
    w.write_ue(0); // slice_type == B
    let bytes = w.finish();
    let result = parse_slice_header(&bytes, HevcNalUnitType::Trail(1), &test_sps(), &test_pps());
    assert!(result.is_err());
}

#[test]
fn parse_rejects_sps_level_rps_flag() {
    let mut w = BitWriter::new();
    w.write_bit(true); // first_slice_segment_in_pic_flag
    w.write_ue(0); // slice_pic_parameter_set_id
    w.write_ue(2); // slice_type == I (no num_ref_idx block needed for this check)
    w.write_bits(5, 8); // poc_lsb
    w.write_bit(true); // short_term_ref_pic_set_sps_flag == 1
    let bytes = w.finish();
    let result = parse_slice_header(&bytes, HevcNalUnitType::Trail(1), &test_sps(), &test_pps());
    assert!(result.is_err());
}

#[test]
fn short_term_ref_pic_set_curr_before_after_poc() {
    let mut s0: SmallVec<[ShortTermRefPicEntry; 8]> = SmallVec::new();
    s0.push(ShortTermRefPicEntry {
        delta_poc: -2,
        used_by_curr_pic: true,
    });
    s0.push(ShortTermRefPicEntry {
        delta_poc: -4,
        used_by_curr_pic: false,
    });
    let mut s1: SmallVec<[ShortTermRefPicEntry; 8]> = SmallVec::new();
    s1.push(ShortTermRefPicEntry {
        delta_poc: 2,
        used_by_curr_pic: false,
    });
    let rps = ShortTermRefPicSet { s0, s1 };

    let (before, after) = rps.curr_before_after_poc(10);
    assert_eq!(before.as_slice(), [8]);
    assert!(after.is_empty());
    assert_eq!(rps.num_curr_pics(), 1);

    let mut all = rps.all_poc(10).into_vec();
    all.sort_unstable();
    assert_eq!(all, vec![6, 8, 12]);
}
