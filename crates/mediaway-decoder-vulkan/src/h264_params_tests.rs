#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use crate::dpb::DpbSlot;

/// Minimal MSB-first bit packer used only to build test SPS/PPS bitstreams;
/// mirrors `mediaway_sw::h264`'s own test-only `BitWriter` (duplicated here
/// to keep this test file self-contained, per that crate's own convention).
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

/// Build a minimal baseline SPS RBSP: `pic_order_cnt_type == 0`,
/// `frame_mbs_only_flag == 1`, no cropping.
fn build_sps_rbsp(
    max_num_ref_frames: u32,
    pic_width_in_mbs_minus1: u32,
    pic_height_in_map_units_minus1: u32,
) -> Vec<u8> {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // seq_parameter_set_id
    writer.write_ue(0); // log2_max_frame_num_minus4
    writer.write_ue(0); // pic_order_cnt_type = 0
    writer.write_ue(0); // log2_max_pic_order_cnt_lsb_minus4
    writer.write_ue(max_num_ref_frames);
    writer.push_bit(0); // gaps_in_frame_num_value_allowed_flag
    writer.write_ue(pic_width_in_mbs_minus1);
    writer.write_ue(pic_height_in_map_units_minus1);
    writer.push_bit(1); // frame_mbs_only_flag
    writer.push_bit(1); // direct_8x8_inference_flag
    writer.push_bit(0); // frame_cropping_flag

    let mut rbsp = vec![66u8, 0, 30]; // Baseline profile, level 3.0
    rbsp.extend(writer.finish());
    rbsp
}

#[test]
fn sps_parse_extracts_expected_fields() {
    // 2 macroblocks wide/tall == 32x32.
    let rbsp = build_sps_rbsp(4, 1, 1);
    let sps = H264Sps::parse(&rbsp).unwrap();
    assert_eq!(sps.profile_idc, 66);
    assert_eq!(sps.level_idc, 30);
    assert_eq!(sps.log2_max_frame_num, 4);
    assert_eq!(sps.max_frame_num, 16);
    assert_eq!(sps.log2_max_pic_order_cnt_lsb, 4);
    assert_eq!(sps.max_num_ref_frames, 4);
    assert_eq!(sps.pic_width_in_mbs, 2);
    assert_eq!(sps.pic_height_in_map_units, 2);
    assert_eq!(sps.width, 32);
    assert_eq!(sps.height, 32);
}

#[test]
fn sps_parse_rejects_nonzero_pic_order_cnt_type() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // seq_parameter_set_id
    writer.write_ue(0); // log2_max_frame_num_minus4
    writer.write_ue(2); // pic_order_cnt_type = 2
    writer.write_ue(1); // max_num_ref_frames
    writer.push_bit(0); // gaps_in_frame_num_value_allowed_flag
    writer.write_ue(1);
    writer.write_ue(1);
    writer.push_bit(1); // frame_mbs_only_flag
    writer.push_bit(1); // direct_8x8_inference_flag
    writer.push_bit(0); // frame_cropping_flag
    let mut rbsp = vec![66u8, 0, 30];
    rbsp.extend(writer.finish());

    let err = H264Sps::parse(&rbsp).unwrap_err();
    assert!(matches!(err, H264ParamError::Unsupported { .. }));
}

#[test]
fn sps_parse_rejects_field_coding() {
    let mut writer = BitWriter::new();
    writer.write_ue(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.write_ue(1);
    writer.push_bit(0);
    writer.write_ue(1);
    writer.write_ue(1);
    writer.push_bit(0); // frame_mbs_only_flag = false
    writer.push_bit(0); // mb_adaptive_frame_field_flag
    writer.push_bit(1); // direct_8x8_inference_flag
    writer.push_bit(0); // frame_cropping_flag
    let mut rbsp = vec![66u8, 0, 30];
    rbsp.extend(writer.finish());

    let err = H264Sps::parse(&rbsp).unwrap_err();
    assert!(matches!(err, H264ParamError::Unsupported { .. }));
}

#[test]
fn sps_to_std_round_trips_key_fields() {
    let rbsp = build_sps_rbsp(2, 4, 4);
    let sps = H264Sps::parse(&rbsp).unwrap();
    let std_sps = sps.to_std();
    assert_eq!(std_sps.max_num_ref_frames, 2);
    assert_eq!(std_sps.pic_width_in_mbs_minus1, 4);
    assert_eq!(std_sps.pic_height_in_map_units_minus1, 4);
    assert_eq!(std_sps.flags.frame_mbs_only_flag(), 1);
    assert_eq!(
        std_sps.chroma_format_idc,
        vulkanalia::vk::video::STD_VIDEO_H264_CHROMA_FORMAT_IDC_420
    );
}

fn build_pps_rbsp(entropy_coding_mode: u32, weighted_pred_flag: u32) -> Vec<u8> {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_ue(0); // seq_parameter_set_id
    writer.push_bit(entropy_coding_mode);
    writer.push_bit(0); // bottom_field_pic_order_in_frame_present_flag
    writer.write_ue(0); // num_slice_groups_minus1
    writer.write_ue(0); // num_ref_idx_l0_default_active_minus1
    writer.write_ue(0); // num_ref_idx_l1_default_active_minus1
    writer.push_bit(weighted_pred_flag);
    writer.write_bits(0, 2); // weighted_bipred_idc
    writer.write_se(0); // pic_init_qp_minus26
    writer.write_se(0); // pic_init_qs_minus26
    writer.write_se(0); // chroma_qp_index_offset
    writer.push_bit(0); // deblocking_filter_control_present_flag
    writer.push_bit(0); // constrained_intra_pred_flag
    writer.push_bit(0); // redundant_pic_cnt_present_flag
    writer.finish()
}

#[test]
fn pps_parse_extracts_expected_fields() {
    let rbsp = build_pps_rbsp(0, 1);
    let pps = H264Pps::parse(&rbsp).unwrap();
    assert!(!pps.entropy_coding_mode);
    assert!(pps.weighted_pred_flag);
    assert_eq!(pps.num_ref_idx_l0_default_active, 1);
    assert_eq!(pps.pic_init_qp, 26);
}

#[test]
fn pps_parse_rejects_multiple_slice_groups() {
    let mut writer = BitWriter::new();
    writer.write_ue(0);
    writer.write_ue(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.write_ue(1); // num_slice_groups_minus1 = 1 -> unsupported
    let rbsp = writer.finish();

    let err = H264Pps::parse(&rbsp).unwrap_err();
    assert!(matches!(err, H264ParamError::Unsupported { .. }));
}

#[test]
fn pps_parse_rejects_redundant_pic_cnt() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_ue(0); // seq_parameter_set_id
    writer.push_bit(0); // entropy_coding_mode_flag
    writer.push_bit(0); // bottom_field_pic_order_in_frame_present_flag
    writer.write_ue(0); // num_slice_groups_minus1
    writer.write_ue(0); // num_ref_idx_l0_default_active_minus1
    writer.write_ue(0); // num_ref_idx_l1_default_active_minus1
    writer.push_bit(0); // weighted_pred_flag
    writer.write_bits(0, 2); // weighted_bipred_idc
    writer.write_se(0); // pic_init_qp_minus26
    writer.write_se(0); // pic_init_qs_minus26
    writer.write_se(0); // chroma_qp_index_offset
    writer.push_bit(0); // deblocking_filter_control_present_flag
    writer.push_bit(0); // constrained_intra_pred_flag
    writer.push_bit(1); // redundant_pic_cnt_present_flag = 1 (unsupported)
    let rbsp = writer.finish();

    let err = H264Pps::parse(&rbsp).unwrap_err();
    assert!(matches!(err, H264ParamError::Unsupported { .. }));
}

#[test]
fn pps_to_std_round_trips_flags() {
    let rbsp = build_pps_rbsp(1, 0);
    let pps = H264Pps::parse(&rbsp).unwrap();
    let std_pps = pps.to_std();
    assert_eq!(std_pps.flags.entropy_coding_mode_flag(), 1);
    assert_eq!(std_pps.flags.weighted_pred_flag(), 0);
}

#[test]
fn derive_pic_order_cnt_msb_no_wrap_for_idr() {
    // IDR reset: prev_msb=0, prev_lsb=0 -> PicOrderCntMsb stays 0.
    assert_eq!(derive_pic_order_cnt_msb(0, 0, 0, 16), 0);
}

#[test]
fn derive_pic_order_cnt_msb_stays_stable_within_half_range() {
    assert_eq!(derive_pic_order_cnt_msb(4, 0, 2, 16), 0);
}

#[test]
fn derive_pic_order_cnt_msb_wraps_forward() {
    // lsb dropped a lot relative to prev_lsb (wrapped past MaxPicOrderCntLsb).
    assert_eq!(derive_pic_order_cnt_msb(1, 0, 15, 16), 16);
}

#[test]
fn derive_pic_order_cnt_msb_wraps_backward() {
    // lsb jumped up a lot relative to prev_lsb (backward wrap case).
    assert_eq!(derive_pic_order_cnt_msb(15, 16, 1, 16), 0);
}

#[test]
fn reference_info_from_slot_carries_frame_num_and_poc() {
    let slot = DpbSlot::new_reference(7, 7, 14);
    let info = reference_info_from_slot(&slot);
    assert_eq!(info.FrameNum, 7);
    assert_eq!(info.PicOrderCnt, [14, 14]);
}
