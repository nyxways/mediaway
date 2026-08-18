//! Pure unit tests for [`super::parse_sps`]/[`super::parse_pps`] against hand-built RBSP
//! bitstreams (constructed with a tiny local Exp-Golomb bit writer — the mirror image of
//! `mediaway_sw::h264::BitReader`, test-only).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{parse_pps, parse_sps};

/// Minimal MSB-first bit writer with Exp-Golomb `ue(v)`/`se(v)` encoding and
/// `rbsp_trailing_bits()` — just enough to build valid H.264 SPS/PPS RBSPs for tests.
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

    /// `rbsp_trailing_bits()`: stop bit, then zero-pad to a byte boundary.
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

fn baseline_sps_bytes(max_num_ref_frames: u32, mb_size: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.write_bits(66, 8); // profile_idc: Baseline (no chroma-format block)
    w.write_bits(0, 8); // constraint flags + reserved
    w.write_bits(30, 8); // level_idc
    w.write_ue(0); // seq_parameter_set_id
    w.write_ue(0); // log2_max_frame_num_minus4 -> log2_max_frame_num == 4
    w.write_ue(0); // pic_order_cnt_type == 0
    w.write_ue(4); // log2_max_pic_order_cnt_lsb_minus4 -> == 8
    w.write_ue(max_num_ref_frames);
    w.write_bit(false); // gaps_in_frame_num_value_allowed_flag
    w.write_ue(mb_size - 1); // pic_width_in_mbs_minus1
    w.write_ue(mb_size - 1); // pic_height_in_map_units_minus1
    w.write_bit(true); // frame_mbs_only_flag
    w.write_bit(true); // direct_8x8_inference_flag
    w.write_bit(false); // frame_cropping_flag
    w.finish()
}

#[test]
fn parse_sps_roundtrips_baseline_fields() {
    let bytes = baseline_sps_bytes(2, 4);
    let sps = parse_sps(&bytes).expect("valid hand-built SPS");
    assert_eq!(sps.profile_idc, 66);
    assert_eq!(sps.level_idc, 30);
    assert_eq!(sps.log2_max_frame_num, 4);
    assert_eq!(sps.pic_order_cnt_type, 0);
    assert_eq!(sps.log2_max_pic_order_cnt_lsb, 8);
    assert_eq!(sps.max_num_ref_frames, 2);
    assert_eq!(sps.mb_width, 4);
    assert_eq!(sps.mb_height, 4);
    assert_eq!(sps.cropped_width, 64);
    assert_eq!(sps.cropped_height, 64);
    assert!(sps.direct_8x8_inference_flag);
}

fn baseline_pps_bytes(with_extension: bool) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.write_ue(0); // pic_parameter_set_id
    w.write_ue(0); // seq_parameter_set_id
    w.write_bit(false); // entropy_coding_mode_flag (CAVLC)
    w.write_bit(false); // bottom_field_pic_order_in_frame_present_flag
    w.write_ue(0); // num_slice_groups_minus1
    w.write_ue(0); // num_ref_idx_l0_default_active_minus1
    w.write_ue(0); // num_ref_idx_l1_default_active_minus1
    w.write_bit(false); // weighted_pred_flag
    w.write_bits(0, 2); // weighted_bipred_idc
    w.write_se(0); // pic_init_qp_minus26
    w.write_se(0); // pic_init_qs_minus26
    w.write_se(0); // chroma_qp_index_offset
    w.write_bit(true); // deblocking_filter_control_present_flag
    w.write_bit(false); // constrained_intra_pred_flag
    w.write_bit(false); // redundant_pic_cnt_present_flag
    if with_extension {
        w.write_bit(true); // transform_8x8_mode_flag
        w.write_bit(false); // pic_scaling_matrix_present_flag
        w.write_se(3); // second_chroma_qp_index_offset
    }
    w.finish()
}

#[test]
fn parse_pps_without_high_profile_extension() {
    let bytes = baseline_pps_bytes(false);
    let pps = parse_pps(&bytes).expect("valid hand-built PPS");
    assert!(pps.deblocking_filter_control_present_flag);
    assert!(!pps.transform_8x8_mode_flag);
    assert_eq!(pps.second_chroma_qp_index_offset, 0);
}

#[test]
fn parse_pps_reads_high_profile_extension_when_more_rbsp_data_present() {
    let bytes = baseline_pps_bytes(true);
    let pps = parse_pps(&bytes).expect("valid hand-built PPS with extension");
    assert!(pps.transform_8x8_mode_flag);
    assert_eq!(pps.second_chroma_qp_index_offset, 3);
}

#[test]
fn parse_pps_rejects_multiple_slice_groups() {
    let mut w = BitWriter::new();
    w.write_ue(0);
    w.write_ue(0);
    w.write_bit(false);
    w.write_bit(false);
    w.write_ue(1); // num_slice_groups_minus1 != 0 -> FMO/ASO, unsupported
    let bytes = w.finish();
    assert!(parse_pps(&bytes).is_err());
}
