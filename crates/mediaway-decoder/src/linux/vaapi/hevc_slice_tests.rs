#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal bit-level writer producing a slice-header-shaped RBSP for round-trip tests (mirrors
/// this crate's H.264 `slice_tests.rs`'s writer; kept file-local per this workspace's
/// sibling-test convention).
#[derive(Default)]
struct BitWriter {
    bits: Vec<u8>,
}

impl BitWriter {
    fn push_bit(&mut self, bit: u8) {
        self.bits.push(bit & 1);
    }

    fn push_bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.push_bit(((value >> i) & 1) as u8);
        }
    }

    fn push_ue(&mut self, value: u32) {
        let value_plus1 = value + 1;
        let num_bits = 32 - value_plus1.leading_zeros();
        for _ in 0..num_bits - 1 {
            self.push_bit(0);
        }
        self.push_bits(value_plus1, num_bits);
    }

    fn push_se(&mut self, value: i32) {
        #[allow(
            clippy::cast_sign_loss,
            reason = "magnitude fits u32 for the small test values used here"
        )]
        let code = if value > 0 {
            2 * value as u32 - 1
        } else {
            (-value) as u32 * 2
        };
        self.push_ue(code);
    }

    fn bit_len(&self) -> usize {
        self.bits.len()
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut out = vec![0u8; self.bits.len().div_ceil(8)];
        for (i, bit) in self.bits.iter().enumerate() {
            if *bit != 0 {
                out[i / 8] |= 1 << (7 - (i % 8));
            }
        }
        out
    }
}

fn test_sps(sao: bool, temporal_mvp: bool) -> HevcSps {
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
        sample_adaptive_offset_enabled_flag: sao,
        sps_temporal_mvp_enabled_flag: temporal_mvp,
        strong_intra_smoothing_enabled_flag: true,
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "each bool names one independent ITU-T H.265 PPS syntax element under test — \
              mirrors this crate's H.264 slice_tests.rs's own struct_excessive_bools allow"
)]
fn test_pps(
    num_extra_slice_header_bits: u32,
    output_flag_present_flag: bool,
    weighted_pred_flag: bool,
    cabac_init_present_flag: bool,
    pps_slice_chroma_qp_offsets_present_flag: bool,
    pps_loop_filter_across_slices_enabled_flag: bool,
) -> HevcPps {
    HevcPps {
        pps_pic_parameter_set_id: 0,
        dependent_slice_segments_enabled_flag: false,
        output_flag_present_flag,
        num_extra_slice_header_bits,
        sign_data_hiding_enabled_flag: false,
        cabac_init_present_flag,
        num_ref_idx_l0_default_active: 1,
        num_ref_idx_l1_default_active: 1,
        init_qp: 26,
        constrained_intra_pred_flag: false,
        transform_skip_enabled_flag: false,
        cu_qp_delta_enabled_flag: false,
        diff_cu_qp_delta_depth: 0,
        pps_cb_qp_offset: 0,
        pps_cr_qp_offset: 0,
        pps_slice_chroma_qp_offsets_present_flag,
        weighted_pred_flag,
        weighted_bipred_flag: false,
        transquant_bypass_enabled_flag: false,
        pps_loop_filter_across_slices_enabled_flag,
        lists_modification_present_flag: false,
        log2_parallel_merge_level_minus2: 0,
    }
}

fn default_sps() -> HevcSps {
    test_sps(false, false)
}

fn default_pps() -> HevcPps {
    test_pps(0, false, false, false, false, false)
}

/// Writes an IDR slice-segment-header RBSP through `byte_alignment()`, for `pps`.
fn idr_rbsp(pps: &HevcPps) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.push_bit(1); // first_slice_segment_in_pic_flag
    w.push_bit(1); // no_output_of_prior_pics_flag (IDR)
    w.push_ue(0); // slice_pic_parameter_set_id
    for _ in 0..pps.num_extra_slice_header_bits {
        w.push_bit(0);
    }
    w.push_ue(2); // slice_type: I
    if pps.output_flag_present_flag {
        w.push_bit(1);
    }
    // sps.sample_adaptive_offset_enabled_flag is always false for these IDR fixtures.
    w.push_se(-2); // slice_qp_delta
    if pps.pps_slice_chroma_qp_offsets_present_flag {
        w.push_se(1);
        w.push_se(-1);
    }
    if pps.pps_loop_filter_across_slices_enabled_flag {
        w.push_bit(1);
    }
    w.push_bit(1); // alignment_bit_equal_to_one
    let pad = (8 - (w.bit_len() % 8)) % 8;
    for _ in 0..pad {
        w.push_bit(0);
    }
    w.into_bytes()
}

#[test]
fn idr_slice_parses_and_byte_aligns() {
    let sps = default_sps();
    let pps = default_pps();
    let rbsp = idr_rbsp(&pps);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, true).expect("IDR parses");
    assert!(matches!(header.slice_type, HevcSliceType::I));
    assert_eq!(header.slice_pic_parameter_set_id, 0);
    assert!(header.pic_order_cnt_lsb.is_none());
    assert!(header.short_term_rps.is_none());
    assert_eq!(header.st_rps_bits, 0);
    assert_eq!(header.slice_qp_delta, -2);
    assert_eq!(header.num_ref_idx_l0_active, 0);
    assert!(header.bits_consumed.is_multiple_of(8));
}

/// Hand-computed bit count: `first_slice_segment_in_pic_flag`(1) +
/// `no_output_of_prior_pics_flag`(1) + `slice_pic_parameter_set_id` ue(0)=1 bit +
/// `slice_type` ue(2)=3 bits ("011") + `slice_qp_delta` se(-2)=5 bits ("00101" for code 4) +
/// `alignment_bit_equal_to_one`(1) = 12 bits, padded to 16 (2 bytes).
#[test]
fn idr_slice_bits_consumed_matches_hand_computed_count() {
    let sps = default_sps();
    let pps = default_pps();
    let rbsp = idr_rbsp(&pps);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, true).expect("IDR parses");
    assert_eq!(header.bits_consumed, 16);
}

/// Writes a single-forward-reference P slice-segment-header RBSP through `byte_alignment()`.
fn p_rbsp(sps: &HevcSps, pps: &HevcPps, poc_lsb: u32) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.push_bit(1); // first_slice_segment_in_pic_flag
    // no no_output_of_prior_pics_flag: not IDR
    w.push_ue(0); // slice_pic_parameter_set_id
    for _ in 0..pps.num_extra_slice_header_bits {
        w.push_bit(0);
    }
    w.push_ue(1); // slice_type: P
    if pps.output_flag_present_flag {
        w.push_bit(1);
    }
    w.push_bits(poc_lsb, sps.log2_max_pic_order_cnt_lsb);
    w.push_bit(0); // short_term_ref_pic_set_sps_flag
    // short_term_ref_pic_set(0): single-forward-reference shape.
    w.push_ue(1); // num_negative_pics
    w.push_ue(0); // num_positive_pics
    w.push_ue(0); // delta_poc_s0_minus1[0] -> delta_poc == -1
    w.push_bit(1); // used_by_curr_pic_s0_flag[0]
    if sps.sps_temporal_mvp_enabled_flag {
        w.push_bit(1); // slice_temporal_mvp_enabled_flag
    }
    if sps.sample_adaptive_offset_enabled_flag {
        w.push_bit(1); // slice_sao_luma_flag
        w.push_bit(0); // slice_sao_chroma_flag
    }
    // is_p_slice branch:
    w.push_bit(0); // num_ref_idx_active_override_flag
    if pps.cabac_init_present_flag {
        w.push_bit(1); // cabac_init_flag
    }
    w.push_ue(3); // five_minus_max_num_merge_cand
    w.push_se(1); // slice_qp_delta
    if pps.pps_slice_chroma_qp_offsets_present_flag {
        w.push_se(1);
        w.push_se(-1);
    }
    if pps.pps_loop_filter_across_slices_enabled_flag {
        w.push_bit(1);
    }
    w.push_bit(1); // alignment_bit_equal_to_one
    let pad = (8 - (w.bit_len() % 8)) % 8;
    for _ in 0..pad {
        w.push_bit(0);
    }
    w.into_bytes()
}

#[test]
fn p_slice_parses_single_forward_reference_shape() {
    let sps = default_sps();
    let pps = default_pps();
    let rbsp = p_rbsp(&sps, &pps, 5);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false).expect("P parses");
    assert!(matches!(header.slice_type, HevcSliceType::P));
    assert_eq!(header.pic_order_cnt_lsb, Some(5));
    let rps = header.short_term_rps.expect("RPS present");
    assert!(rps.is_single_forward_reference());
    assert_eq!(header.num_ref_idx_l0_active, 1);
    assert_eq!(header.slice_qp_delta, 1);
    assert_eq!(header.five_minus_max_num_merge_cand, 3);
}

/// Hand-computed `st_rps_bits`: `num_negative_pics` ue(1)=3 bits ("010") + `num_positive_pics`
/// ue(0)=1 bit + `delta_poc_s0_minus1[0]` ue(0)=1 bit + `used_by_curr_pic_s0_flag[0]`=1 bit = 6.
#[test]
fn p_slice_st_rps_bits_matches_hand_computed_count() {
    let sps = default_sps();
    let pps = default_pps();
    let rbsp = p_rbsp(&sps, &pps, 5);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false).expect("P parses");
    assert_eq!(header.st_rps_bits, 6);
}

#[test]
fn temporal_mvp_bit_is_read_only_when_sps_flag_set_and_not_idr() {
    let sps = test_sps(false, true);
    let pps = default_pps();
    let rbsp = p_rbsp(&sps, &pps, 3);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false).expect("P parses");
    assert!(header.slice_temporal_mvp_enabled_flag);
}

#[test]
fn sao_flags_are_read_only_when_sps_flag_set() {
    let sps = test_sps(true, false);
    let pps = default_pps();
    let rbsp = p_rbsp(&sps, &pps, 3);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false).expect("P parses");
    assert!(header.slice_sao_luma_flag);
    assert!(!header.slice_sao_chroma_flag);
}

#[test]
fn extra_slice_header_bits_are_skipped() {
    let sps = default_sps();
    let pps = test_pps(3, false, false, false, false, false);
    let rbsp = idr_rbsp(&pps);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, true).expect("IDR parses");
    assert!(matches!(header.slice_type, HevcSliceType::I));
}

#[test]
fn output_flag_present_bit_is_skipped_when_set() {
    let sps = default_sps();
    let pps = test_pps(0, true, false, false, false, false);
    let rbsp = idr_rbsp(&pps);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, true).expect("IDR parses");
    assert!(matches!(header.slice_type, HevcSliceType::I));
}

#[test]
fn cabac_init_flag_is_read_only_when_pps_flag_set() {
    let sps = default_sps();
    let pps = test_pps(0, false, false, true, false, false);
    let rbsp = p_rbsp(&sps, &pps, 3);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false).expect("P parses");
    // p_rbsp writes cabac_init_flag == 1 whenever pps.cabac_init_present_flag is set — assert
    // both that the flag round-trips and that the parser stayed correctly aligned through the
    // rest of the header rather than desyncing.
    assert!(header.cabac_init_flag);
    assert_eq!(header.slice_qp_delta, 1);
}

#[test]
fn chroma_qp_offsets_are_read_only_when_pps_flag_set() {
    let sps = default_sps();
    let pps = test_pps(0, false, false, false, true, false);
    let rbsp = idr_rbsp(&pps);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, true).expect("IDR parses");
    assert_eq!(header.slice_cb_qp_offset, 1);
    assert_eq!(header.slice_cr_qp_offset, -1);
}

#[test]
fn loop_filter_across_slices_flag_is_read_only_when_pps_flag_set() {
    let sps = default_sps();
    let pps = test_pps(0, false, false, false, false, true);
    let rbsp = idr_rbsp(&pps);
    let mut r = BitReader::new(&rbsp);
    let header = HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, true).expect("IDR parses");
    assert!(header.slice_loop_filter_across_slices_enabled_flag);
}

#[test]
fn rejects_non_first_slice_segment() {
    let mut w = BitWriter::default();
    w.push_bit(0); // first_slice_segment_in_pic_flag
    let rbsp = w.into_bytes();
    let mut r = BitReader::new(&rbsp);
    assert_eq!(
        HevcSliceSegmentHeader::parse(&mut r, &default_sps(), &default_pps(), true),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_b_slice() {
    let mut w = BitWriter::default();
    w.push_bit(1); // first_slice_segment_in_pic_flag
    w.push_bit(1); // no_output_of_prior_pics_flag
    w.push_ue(0); // slice_pic_parameter_set_id
    w.push_ue(0); // slice_type: B
    let rbsp = w.into_bytes();
    let mut r = BitReader::new(&rbsp);
    assert_eq!(
        HevcSliceSegmentHeader::parse(&mut r, &default_sps(), &default_pps(), true),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_short_term_ref_pic_set_sps_flag_set() {
    let sps = default_sps();
    let pps = default_pps();
    let mut w = BitWriter::default();
    w.push_bit(1); // first_slice_segment_in_pic_flag
    w.push_ue(0); // slice_pic_parameter_set_id
    w.push_ue(1); // slice_type: P
    w.push_bits(0, sps.log2_max_pic_order_cnt_lsb);
    w.push_bit(1); // short_term_ref_pic_set_sps_flag
    let rbsp = w.into_bytes();
    let mut r = BitReader::new(&rbsp);
    assert_eq!(
        HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_rps_shape_with_two_negative_references() {
    let sps = default_sps();
    let pps = default_pps();
    let mut w = BitWriter::default();
    w.push_bit(1); // first_slice_segment_in_pic_flag
    w.push_ue(0); // slice_pic_parameter_set_id
    w.push_ue(1); // slice_type: P
    w.push_bits(0, sps.log2_max_pic_order_cnt_lsb);
    w.push_bit(0); // short_term_ref_pic_set_sps_flag
    w.push_ue(2); // num_negative_pics: 2 -- rejected, not single-forward-reference
    w.push_ue(0); // num_positive_pics
    w.push_ue(0); // delta_poc_s0_minus1[0]
    w.push_bit(1); // used_by_curr_pic_s0_flag[0]
    w.push_ue(0); // delta_poc_s0_minus1[1] (a real second entry — otherwise the parser would hit
    w.push_bit(1); // used_by_curr_pic_s0_flag[1]  truncated data before shape validation runs)
    let rbsp = w.into_bytes();
    let mut r = BitReader::new(&rbsp);
    assert_eq!(
        HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_rps_entry_not_used_by_curr_pic() {
    let sps = default_sps();
    let pps = default_pps();
    let mut w = BitWriter::default();
    w.push_bit(1);
    w.push_ue(0);
    w.push_ue(1);
    w.push_bits(0, sps.log2_max_pic_order_cnt_lsb);
    w.push_bit(0);
    w.push_ue(1); // num_negative_pics
    w.push_ue(0); // num_positive_pics
    w.push_ue(0); // delta_poc_s0_minus1[0] -> delta_poc == -1
    w.push_bit(0); // used_by_curr_pic_s0_flag[0] == false -- rejected
    let rbsp = w.into_bytes();
    let mut r = BitReader::new(&rbsp);
    assert_eq!(
        HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_p_slice_num_ref_idx_l0_active_override_to_more_than_one() {
    let sps = default_sps();
    let pps = default_pps();
    let mut w = BitWriter::default();
    w.push_bit(1);
    w.push_ue(0);
    w.push_ue(1); // P
    w.push_bits(0, sps.log2_max_pic_order_cnt_lsb);
    w.push_bit(0);
    w.push_ue(1);
    w.push_ue(0);
    w.push_ue(0);
    w.push_bit(1);
    w.push_bit(1); // num_ref_idx_active_override_flag
    w.push_ue(1); // num_ref_idx_l0_active_minus1 -> 2 active refs, rejected
    let rbsp = w.into_bytes();
    let mut r = BitReader::new(&rbsp);
    assert_eq!(
        HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_p_slice_with_weighted_pred_flag_set() {
    let sps = default_sps();
    let pps = test_pps(0, false, true, false, false, false);
    let mut w = BitWriter::default();
    w.push_bit(1);
    w.push_ue(0);
    w.push_ue(1); // P
    w.push_bits(0, sps.log2_max_pic_order_cnt_lsb);
    w.push_bit(0);
    w.push_ue(1);
    w.push_ue(0);
    w.push_ue(0);
    w.push_bit(1);
    let rbsp = w.into_bytes();
    let mut r = BitReader::new(&rbsp);
    assert_eq!(
        HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, false),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_alignment_bit_not_equal_to_one() {
    let sps = default_sps();
    let pps = default_pps();
    let mut w = BitWriter::default();
    w.push_bit(1); // first_slice_segment_in_pic_flag
    w.push_bit(1); // no_output_of_prior_pics_flag
    w.push_ue(0); // slice_pic_parameter_set_id
    w.push_ue(2); // slice_type: I
    w.push_se(0); // slice_qp_delta
    w.push_bit(0); // alignment_bit_equal_to_one violated
    let rbsp = w.into_bytes();
    let mut r = BitReader::new(&rbsp);
    assert_eq!(
        HevcSliceSegmentHeader::parse(&mut r, &sps, &pps, true),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn short_term_ref_pic_set_rejects_more_than_eight_negative_pics() {
    let mut w = BitWriter::default();
    w.push_ue(9); // num_negative_pics
    w.push_ue(0); // num_positive_pics
    let rbsp = w.into_bytes();
    let mut r = BitReader::new(&rbsp);
    assert_eq!(
        ShortTermRefPicSet::parse(&mut r),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn is_single_forward_reference_accepts_only_the_exact_shape() {
    let mut rps = ShortTermRefPicSet::default();
    rps.s0.push(ShortTermRefPicEntry {
        delta_poc: -1,
        used_by_curr_pic: true,
    });
    assert!(rps.is_single_forward_reference());

    rps.s0[0].delta_poc = -2;
    assert!(!rps.is_single_forward_reference());

    rps.s0[0].delta_poc = -1;
    rps.s0[0].used_by_curr_pic = false;
    assert!(!rps.is_single_forward_reference());

    rps.s0[0].used_by_curr_pic = true;
    rps.s1.push(ShortTermRefPicEntry {
        delta_poc: 1,
        used_by_curr_pic: true,
    });
    assert!(!rps.is_single_forward_reference());
}
