//! Pure unit tests for [`super::parse_slice_header`] against hand-built RBSP
//! bitstreams (same tiny Exp-Golomb bit writer approach as `h264_sps_pps_tests.rs`,
//! test-only, duplicated rather than shared across sibling test files).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{RefPicListModOp, SliceType, bit_offset_to_slice_data, parse_slice_header};
use crate::windows::d3d12_video_decode::h264_sps_pps::{Pps, Sps};
use mediaway_sw::h264::NalUnitType;

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

    fn write_se(&mut self, value: i32) {
        #[allow(
            clippy::cast_sign_loss,
            reason = "magnitude-only cast after explicit sign branch, test helper"
        )]
        let k = if value <= 0 {
            (-value as u32) * 2
        } else {
            (value as u32) * 2 - 1
        };
        self.write_ue(k);
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

fn sps_fixture() -> Sps {
    Sps {
        log2_max_frame_num: 4,
        pic_order_cnt_type: 0,
        log2_max_pic_order_cnt_lsb: 8,
        ..Sps::default()
    }
}

fn pps_fixture() -> Pps {
    Pps {
        num_ref_idx_l0_default_active_minus1: 1,
        ..Pps::default()
    }
}

#[test]
fn parse_idr_i_slice() {
    let mut w = BitWriter::new();
    w.write_ue(0); // first_mb_in_slice
    w.write_ue(7); // slice_type: 7 % 5 == 2 == I
    w.write_ue(0); // pic_parameter_set_id
    w.write_bits(0, 4); // frame_num
    w.write_ue(0); // idr_pic_id
    w.write_bits(0, 8); // pic_order_cnt_lsb
    // redundant_pic_cnt_present_flag == false: nothing
    // I slice: no direct_spatial_mv_pred_flag, no num_ref_idx override, no ref_pic_list_mod
    // no_output_of_prior_pics_flag, long_term_reference_flag (nal_ref_idc != 0, IDR)
    w.write_bit(false);
    w.write_bit(false);
    // entropy_coding_mode_flag == false: no cabac_init_idc
    w.write_se(0); // slice_qp_delta
    // deblocking_filter_control_present_flag == false: nothing
    let bytes = w.finish();

    let sps = sps_fixture();
    let pps = pps_fixture();
    let (sh, _bits_read) = parse_slice_header(&bytes, NalUnitType::IdrSlice, 1, &sps, &pps)
        .expect("valid hand-built IDR slice header");
    assert_eq!(sh.slice_type, SliceType::I);
    assert_eq!(sh.frame_num, 0);
    assert_eq!(sh.idr_pic_id, Some(0));
    assert_eq!(sh.pic_order_cnt_lsb, 0);
    assert!(!sh.no_output_of_prior_pics_flag);
    assert_eq!(sh.slice_qp_delta, 0);
}

#[test]
fn parse_p_slice_with_ref_pic_list_modification_and_deblocking() {
    let mut w = BitWriter::new();
    w.write_ue(0); // first_mb_in_slice
    w.write_ue(5); // slice_type: 5 % 5 == 0 == P
    w.write_ue(0); // pic_parameter_set_id
    w.write_bits(3, 4); // frame_num
    w.write_bits(6, 8); // pic_order_cnt_lsb
    // redundant_pic_cnt_present_flag == false
    // P slice: no direct_spatial_mv_pred_flag
    w.write_bit(false); // num_ref_idx_active_override_flag
    // ref_pic_list_modification (L0 only, P slice)
    w.write_bit(true); // ref_pic_list_modification_flag_l0
    w.write_ue(0); // modification_of_pic_nums_idc == 0 (subtract)
    w.write_ue(0); // abs_diff_pic_num_minus1
    w.write_ue(3); // end marker
    // weighted_pred_flag == false: no pred_weight_table
    // nal_ref_idc != 0, not IDR: adaptive_ref_pic_marking_mode_flag
    w.write_bit(false);
    // entropy_coding_mode_flag == false: no cabac_init_idc
    w.write_se(2); // slice_qp_delta
    // deblocking_filter_control_present_flag == true
    w.write_ue(0); // disable_deblocking_filter_idc (!= 1 -> read offsets)
    w.write_se(1); // slice_alpha_c0_offset_div2
    w.write_se(-1); // slice_beta_offset_div2
    let bytes = w.finish();

    let sps = sps_fixture();
    let pps = Pps {
        deblocking_filter_control_present_flag: true,
        ..pps_fixture()
    };
    let (sh, _bits_read) = parse_slice_header(&bytes, NalUnitType::NonIdrSlice, 1, &sps, &pps)
        .expect("valid hand-built P slice header");
    assert_eq!(sh.slice_type, SliceType::P);
    assert_eq!(sh.frame_num, 3);
    assert_eq!(sh.pic_order_cnt_lsb, 6);
    assert_eq!(sh.num_ref_idx_l0_active_minus1, 1); // from PPS default, no override
    assert_eq!(
        sh.ref_pic_list_modification_l0.as_slice(),
        &[RefPicListModOp {
            add: false,
            abs_diff_pic_num_minus1: 0,
        }]
    );
    assert_eq!(sh.slice_qp_delta, 2);
    assert_eq!(sh.disable_deblocking_filter_idc, 0);
    assert_eq!(sh.slice_alpha_c0_offset_div2, 1);
    assert_eq!(sh.slice_beta_offset_div2, -1);
}

#[test]
fn parse_rejects_adaptive_ref_pic_marking() {
    let mut w = BitWriter::new();
    w.write_ue(0);
    w.write_ue(5); // P
    w.write_ue(0);
    w.write_bits(1, 4);
    w.write_bits(0, 8);
    w.write_bit(false); // num_ref_idx_active_override_flag
    w.write_bit(false); // ref_pic_list_modification_flag_l0
    w.write_bit(true); // adaptive_ref_pic_marking_mode_flag == true -> MMCO, rejected
    let bytes = w.finish();

    let sps = sps_fixture();
    let pps = pps_fixture();
    let err = parse_slice_header(&bytes, NalUnitType::NonIdrSlice, 1, &sps, &pps)
        .expect_err("adaptive marking must be rejected");
    assert_eq!(err, crate::DecodeError::Unsupported);
}

#[test]
fn parse_rejects_explicit_weighted_prediction() {
    let mut w = BitWriter::new();
    w.write_ue(0);
    w.write_ue(5); // P
    w.write_ue(0);
    w.write_bits(1, 4);
    w.write_bits(0, 8);
    w.write_bit(false); // num_ref_idx_active_override_flag
    w.write_bit(false); // ref_pic_list_modification_flag_l0
    let bytes = w.finish();

    let sps = sps_fixture();
    let pps = Pps {
        weighted_pred_flag: true,
        ..pps_fixture()
    };
    let err = parse_slice_header(&bytes, NalUnitType::NonIdrSlice, 1, &sps, &pps)
        .expect_err("explicit weighted prediction must be rejected");
    assert_eq!(err, crate::DecodeError::Unsupported);
}

#[test]
fn bit_offset_to_slice_data_passes_through_for_cavlc() {
    // CAVLC (entropy_coding_mode_flag == false): DXVA_H264.pdf's BitOffsetToSliceData
    // is the de-emulated RBSP bit count itself, no rounding required.
    assert_eq!(bit_offset_to_slice_data(37, false), 37);
    assert_eq!(bit_offset_to_slice_data(0, false), 0);
}

#[test]
fn bit_offset_to_slice_data_byte_aligns_for_cabac() {
    // CABAC (entropy_coding_mode_flag == true): the spec requires
    // `BitOffsetToSliceData % 8 == 0` (rounds past cabac_alignment_one_bit()).
    assert_eq!(bit_offset_to_slice_data(33, true), 40);
    assert_eq!(bit_offset_to_slice_data(40, true), 40); // already aligned
    assert_eq!(bit_offset_to_slice_data(1, true), 8);
}
