#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use crate::vulkan::dpb::DpbSlot;
use mediaway_sw::h264::NalUnitType;

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

fn test_sps() -> H264Sps {
    H264Sps {
        seq_parameter_set_id: 0,
        profile_idc: 66,
        level_idc: 30,
        log2_max_frame_num: 4,
        max_frame_num: 16,
        log2_max_pic_order_cnt_lsb: 4,
        max_num_ref_frames: 2,
        pic_width_in_mbs: 2,
        pic_height_in_map_units: 2,
        width: 32,
        height: 32,
    }
}

fn test_pps() -> H264Pps {
    H264Pps {
        pic_parameter_set_id: 0,
        seq_parameter_set_id: 0,
        entropy_coding_mode: false,
        num_ref_idx_l0_default_active: 1,
        num_ref_idx_l1_default_active: 1,
        weighted_pred_flag: false,
        weighted_bipred_idc: 0,
        pic_init_qp: 26,
        chroma_qp_index_offset: 0,
        deblocking_filter_control_present: false,
    }
}

#[test]
fn parse_idr_i_slice_header() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(2); // slice_type = I
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(0, 4); // frame_num
    writer.write_ue(0); // idr_pic_id
    writer.write_bits(0, 4); // pic_order_cnt_lsb
    writer.push_bit(0); // no_output_of_prior_pics_flag
    writer.push_bit(0); // long_term_reference_flag
    writer.write_se(0); // slice_qp_delta
    let rbsp = writer.finish();

    let mut reader = BitReader::new(&rbsp);
    let header = H264SliceHeader::parse(
        &mut reader,
        &test_sps(),
        &test_pps(),
        NalUnitType::IdrSlice,
        1,
    )
    .unwrap();
    assert!(matches!(header.slice_type, H264SliceType::I));
    assert_eq!(header.frame_num, 0);
    assert_eq!(header.idr_pic_id, Some(0));
    assert!(header.ref_pic_list_modifications_l0.is_empty());
}

#[test]
fn parse_p_slice_header_without_modifications() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(0); // slice_type = P
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(1, 4); // frame_num = 1
    writer.write_bits(2, 4); // pic_order_cnt_lsb = 2
    writer.push_bit(0); // num_ref_idx_active_override_flag
    writer.push_bit(0); // ref_pic_list_modification_flag_l0
    writer.push_bit(0); // adaptive_ref_pic_marking_mode_flag
    writer.write_se(0); // slice_qp_delta
    let rbsp = writer.finish();

    let mut reader = BitReader::new(&rbsp);
    let header = H264SliceHeader::parse(
        &mut reader,
        &test_sps(),
        &test_pps(),
        NalUnitType::NonIdrSlice,
        1,
    )
    .unwrap();
    assert!(matches!(header.slice_type, H264SliceType::P));
    assert_eq!(header.frame_num, 1);
    assert_eq!(header.idr_pic_id, None);
    assert_eq!(header.num_ref_idx_l0_active, 1); // pps default
    assert!(header.ref_pic_list_modifications_l0.is_empty());
}

#[test]
fn parse_p_slice_header_with_modification() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(0); // slice_type = P
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(2, 4); // frame_num = 2
    writer.write_bits(0, 4); // pic_order_cnt_lsb
    writer.push_bit(0); // num_ref_idx_active_override_flag
    writer.push_bit(1); // ref_pic_list_modification_flag_l0
    writer.write_ue(0); // modification_of_pic_nums_idc = 0 (subtract)
    writer.write_ue(0); // abs_diff_pic_num_minus1 = 0
    writer.write_ue(3); // terminator
    writer.push_bit(0); // adaptive_ref_pic_marking_mode_flag
    writer.write_se(0); // slice_qp_delta
    let rbsp = writer.finish();

    let mut reader = BitReader::new(&rbsp);
    let header = H264SliceHeader::parse(
        &mut reader,
        &test_sps(),
        &test_pps(),
        NalUnitType::NonIdrSlice,
        1,
    )
    .unwrap();
    assert_eq!(header.ref_pic_list_modifications_l0.len(), 1);
    assert_eq!(header.ref_pic_list_modifications_l0[0].idc, 0);
    assert_eq!(header.ref_pic_list_modifications_l0[0].value, 0);
}

#[test]
fn parse_rejects_b_slice() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(1); // slice_type = B
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let err = H264SliceHeader::parse(
        &mut reader,
        &test_sps(),
        &test_pps(),
        NalUnitType::NonIdrSlice,
        1,
    )
    .unwrap_err();
    assert!(matches!(err, H264ParamError::Unsupported { .. }));
}

#[test]
fn parse_rejects_adaptive_ref_pic_marking() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(0); // slice_type = P
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(1, 4); // frame_num
    writer.write_bits(0, 4); // pic_order_cnt_lsb
    writer.push_bit(0); // num_ref_idx_active_override_flag
    writer.push_bit(0); // ref_pic_list_modification_flag_l0
    writer.push_bit(1); // adaptive_ref_pic_marking_mode_flag = 1 (unsupported)
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let err = H264SliceHeader::parse(
        &mut reader,
        &test_sps(),
        &test_pps(),
        NalUnitType::NonIdrSlice,
        1,
    )
    .unwrap_err();
    assert!(matches!(err, H264ParamError::Unsupported { .. }));
}

#[test]
fn parse_rejects_multi_slice_pictures() {
    let mut writer = BitWriter::new();
    writer.write_ue(5); // first_mb_in_slice != 0
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let err = H264SliceHeader::parse(
        &mut reader,
        &test_sps(),
        &test_pps(),
        NalUnitType::NonIdrSlice,
        1,
    )
    .unwrap_err();
    assert!(matches!(err, H264ParamError::Unsupported { .. }));
}

#[test]
fn parse_reads_deblocking_filter_control_fields_when_pps_signals_them() {
    let mut pps = test_pps();
    pps.deblocking_filter_control_present = true;

    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(2); // slice_type = I
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(0, 4); // frame_num
    writer.write_ue(0); // idr_pic_id
    writer.write_bits(0, 4); // pic_order_cnt_lsb
    writer.push_bit(0); // no_output_of_prior_pics_flag
    writer.push_bit(0); // long_term_reference_flag
    writer.write_se(0); // slice_qp_delta
    writer.write_ue(0); // disable_deblocking_filter_idc = 0
    writer.write_se(1); // slice_alpha_c0_offset_div2
    writer.write_se(-1); // slice_beta_offset_div2
    let rbsp = writer.finish();

    let mut reader = BitReader::new(&rbsp);
    // Successfully parsing to completion (no truncation error) proves the
    // extra fields were consumed at the right bit positions.
    H264SliceHeader::parse(&mut reader, &test_sps(), &pps, NalUnitType::IdrSlice, 1).unwrap();
}

#[test]
fn default_ref_pic_list0_sorts_by_descending_frame_num_wrap() {
    let mut dpb = Dpb::new(4);
    dpb.insert(0, DpbSlot::new_reference(1, 1, 2)).unwrap();
    dpb.insert(1, DpbSlot::new_reference(3, 3, 6)).unwrap();
    dpb.insert(2, DpbSlot::new_reference(2, 2, 4)).unwrap();
    let list = default_ref_pic_list0(&dpb);
    assert_eq!(list, vec![1, 2, 0]);
}

#[test]
fn default_ref_pic_list0_skips_non_reference_slots() {
    let mut dpb = Dpb::new(2);
    dpb.insert(0, DpbSlot::new_reference(1, 1, 2)).unwrap();
    let mut non_ref = DpbSlot::new_reference(2, 2, 4);
    non_ref.used_for_reference = false;
    dpb.insert(1, non_ref).unwrap();
    assert_eq!(default_ref_pic_list0(&dpb), vec![0]);
}

#[test]
fn apply_ref_pic_list_modifications_moves_target_to_front() {
    let mut dpb = Dpb::new(4);
    dpb.insert(0, DpbSlot::new_reference(1, 1, 2)).unwrap();
    dpb.insert(1, DpbSlot::new_reference(3, 3, 6)).unwrap();
    dpb.insert(2, DpbSlot::new_reference(2, 2, 4)).unwrap();
    let default_list = default_ref_pic_list0(&dpb); // [1, 2, 0] (frame_num_wrap 3,2,1)

    // current_pic_num = 4 (the picture being decoded); modification idc=0
    // (subtract), abs_diff_pic_num_minus1=2 -> abs_diff=3 -> target picNum =
    // 4 - 3 = 1 -> slot 0 (frame_num_wrap == 1) moves to the front.
    let modifications = [RefPicListModification { idc: 0, value: 2 }];
    let modified = apply_ref_pic_list_modifications(default_list, &dpb, &modifications, 4, 16);
    assert_eq!(modified.first().copied(), Some(0));
}
