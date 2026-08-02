#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal MSB-first bit packer used only to build test PPS bitstreams; mirrors the bit
/// order [`BitReader`] expects (same helper as `bitreader_tests.rs`, duplicated here to
/// keep each test file self-contained).
struct BitWriter {
    bytes: Vec<u8>,
    cur: u8,
    nbits: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            cur: 0,
            nbits: 0,
        }
    }

    fn push_bit(&mut self, bit: u32) {
        let bit_u8 = u8::from(bit & 1 == 1);
        self.cur = (self.cur << 1) | bit_u8;
        self.nbits += 1;
        if self.nbits == 8 {
            self.bytes.push(self.cur);
            self.cur = 0;
            self.nbits = 0;
        }
    }

    fn write_bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.push_bit(value >> i);
        }
    }

    fn write_ue(&mut self, value: u32) {
        let code = value + 1;
        let len = u32::BITS - code.leading_zeros();
        for _ in 0..(len - 1) {
            self.push_bit(0);
        }
        self.write_bits(code, len);
    }

    fn write_se(&mut self, value: i32) {
        let magnitude = value.unsigned_abs();
        let code = if value <= 0 {
            magnitude * 2
        } else {
            magnitude * 2 - 1
        };
        self.write_ue(code);
    }

    fn finish(mut self) -> Vec<u8> {
        while self.nbits != 0 {
            self.push_bit(0);
        }
        self.bytes
    }
}

#[test]
fn parse_extracts_common_fields_single_slice_group() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_ue(0); // seq_parameter_set_id
    writer.push_bit(1); // entropy_coding_mode_flag = CABAC
    writer.push_bit(0); // bottom_field_pic_order_in_frame_present_flag
    writer.write_ue(0); // num_slice_groups_minus1 = 0 (single slice group)
    writer.write_ue(0); // num_ref_idx_l0_default_active_minus1
    writer.write_ue(0); // num_ref_idx_l1_default_active_minus1
    writer.push_bit(0); // weighted_pred_flag
    writer.write_bits(0, 2); // weighted_bipred_idc
    writer.write_se(0); // pic_init_qp_minus26
    writer.write_se(0); // pic_init_qs_minus26
    writer.write_se(0); // chroma_qp_index_offset
    writer.push_bit(1); // deblocking_filter_control_present_flag
    writer.push_bit(0); // constrained_intra_pred_flag
    writer.push_bit(0); // redundant_pic_cnt_present_flag

    let rbsp = writer.finish();
    let pps = Pps::parse(&rbsp).unwrap();

    assert_eq!(pps.pic_parameter_set_id, 0);
    assert_eq!(pps.seq_parameter_set_id, 0);
    assert!(pps.entropy_coding_mode);
    assert_eq!(pps.num_ref_idx_l0_default_active, 1);
    assert_eq!(pps.num_ref_idx_l1_default_active, 1);
    assert_eq!(pps.pic_init_qp, 26);
    assert_eq!(pps.chroma_qp_index_offset, 0);
    assert!(pps.deblocking_filter_control_present);
    assert!(!pps.constrained_intra_pred);
}

#[test]
fn parse_computes_plus_one_and_plus_26_offsets_correctly() {
    let mut writer = BitWriter::new();
    writer.write_ue(3); // pic_parameter_set_id
    writer.write_ue(1); // seq_parameter_set_id
    writer.push_bit(0); // entropy_coding_mode_flag = CAVLC
    writer.push_bit(0); // bottom_field_pic_order_in_frame_present_flag
    writer.write_ue(0); // num_slice_groups_minus1 = 0
    writer.write_ue(15); // num_ref_idx_l0_default_active_minus1 -> 16
    writer.write_ue(3); // num_ref_idx_l1_default_active_minus1 -> 4
    writer.push_bit(0); // weighted_pred_flag
    writer.write_bits(0, 2); // weighted_bipred_idc
    writer.write_se(-5); // pic_init_qp_minus26 -> 21
    writer.write_se(0); // pic_init_qs_minus26
    writer.write_se(0); // chroma_qp_index_offset
    writer.push_bit(0); // deblocking_filter_control_present_flag
    writer.push_bit(1); // constrained_intra_pred_flag
    writer.push_bit(0); // redundant_pic_cnt_present_flag

    let rbsp = writer.finish();
    let pps = Pps::parse(&rbsp).unwrap();

    assert_eq!(pps.pic_parameter_set_id, 3);
    assert_eq!(pps.seq_parameter_set_id, 1);
    assert!(!pps.entropy_coding_mode);
    assert_eq!(pps.num_ref_idx_l0_default_active, 16);
    assert_eq!(pps.num_ref_idx_l1_default_active, 4);
    assert_eq!(pps.pic_init_qp, 21);
    assert!(!pps.deblocking_filter_control_present);
    assert!(pps.constrained_intra_pred);
}

#[test]
fn parse_errors_on_multiple_slice_groups() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_ue(0); // seq_parameter_set_id
    writer.push_bit(0); // entropy_coding_mode_flag
    writer.push_bit(0); // bottom_field_pic_order_in_frame_present_flag
    writer.write_ue(1); // num_slice_groups_minus1 = 1 (two slice groups / FMO)

    let rbsp = writer.finish();
    assert_eq!(Pps::parse(&rbsp), Err(H264Error::SliceGroupsUnsupported));
}

#[test]
fn parse_errors_on_empty_input() {
    assert_eq!(Pps::parse(&[]), Err(H264Error::UnexpectedEof));
}

#[test]
fn parse_errors_on_truncated_input_mid_field() {
    // Enough for pic_parameter_set_id + seq_parameter_set_id + entropy_coding_mode_flag,
    // but cut off before num_slice_groups_minus1 can be read.
    let mut writer = BitWriter::new();
    writer.write_ue(0);
    writer.write_ue(0);
    writer.push_bit(0);
    writer.push_bit(0);
    // Deliberately stop here: no bits left for num_slice_groups_minus1's ue(v) prefix.
    let rbsp = writer.finish();
    assert_eq!(Pps::parse(&rbsp), Err(H264Error::UnexpectedEof));
}
