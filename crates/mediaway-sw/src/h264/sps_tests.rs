#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal MSB-first bit packer used only to build test SPS/PPS bitstreams; mirrors the
/// bit order [`BitReader`] expects (same helper as `bitreader_tests.rs`, duplicated here
/// to keep each test file self-contained).
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

/// Build an SPS RBSP: 3 fixed header bytes (profile/constraints/level) followed by the
/// bit-packed body, up to (and optionally including) `frame_cropping`.
fn build_baseline_sps_rbsp(
    profile_idc: u8,
    level_idc: u8,
    pic_width_in_mbs_minus1: u32,
    pic_height_in_map_units_minus1: u32,
    crop: Option<(u32, u32, u32, u32)>,
) -> Vec<u8> {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // seq_parameter_set_id
    writer.write_ue(0); // log2_max_frame_num_minus4
    writer.write_ue(0); // pic_order_cnt_type = 0
    writer.write_ue(0); // log2_max_pic_order_cnt_lsb_minus4
    writer.write_ue(1); // max_num_ref_frames
    writer.push_bit(0); // gaps_in_frame_num_value_allowed_flag
    writer.write_ue(pic_width_in_mbs_minus1);
    writer.write_ue(pic_height_in_map_units_minus1);
    writer.push_bit(1); // frame_mbs_only_flag = true
    writer.push_bit(1); // direct_8x8_inference_flag
    match crop {
        Some((left, right, top, bottom)) => {
            writer.push_bit(1); // frame_cropping_flag
            writer.write_ue(left);
            writer.write_ue(right);
            writer.write_ue(top);
            writer.write_ue(bottom);
        }
        None => writer.push_bit(0), // frame_cropping_flag = false
    }

    let mut rbsp = vec![profile_idc, 0, level_idc];
    rbsp.extend(writer.finish());
    rbsp
}

#[test]
fn parse_extracts_width_height_profile_level_for_baseline_no_crop() {
    // 320x240: pic_width_in_mbs_minus1=19 (20*16=320), pic_height_in_map_units_minus1=14 (15*16=240).
    let rbsp = build_baseline_sps_rbsp(66, 30, 19, 14, None);
    let sps = Sps::parse(&rbsp).unwrap();
    assert_eq!(sps.profile_idc, 66);
    assert_eq!(sps.level_idc, 30);
    assert_eq!(sps.constraint_flags, 0);
    assert_eq!(sps.width, 320);
    assert_eq!(sps.height, 240);
    assert!(sps.frame_mbs_only);
    assert_eq!(sps.chroma_format_idc, 1);
    assert_eq!(sps.log2_max_frame_num, 4);
    assert_eq!(sps.pic_order_cnt_type, 0);
    assert_eq!(sps.log2_max_pic_order_cnt_lsb, 4);
    assert_eq!(sps.pic_width_in_mbs, 20);
    assert_eq!(sps.pic_height_in_mbs, 15);
}

#[test]
fn parse_extracts_log2_frame_num_and_poc_lsb_with_nonzero_minus4_values() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // seq_parameter_set_id
    writer.write_ue(2); // log2_max_frame_num_minus4 -> log2_max_frame_num = 6
    writer.write_ue(0); // pic_order_cnt_type = 0
    writer.write_ue(3); // log2_max_pic_order_cnt_lsb_minus4 -> 7
    writer.write_ue(1); // max_num_ref_frames
    writer.push_bit(0); // gaps_in_frame_num_value_allowed_flag
    writer.write_ue(0); // pic_width_in_mbs_minus1 (1 MB wide)
    writer.write_ue(0); // pic_height_in_map_units_minus1 (1 MB tall)
    writer.push_bit(1); // frame_mbs_only_flag
    writer.push_bit(1); // direct_8x8_inference_flag
    writer.push_bit(0); // frame_cropping_flag

    let mut rbsp = vec![66u8, 0, 30];
    rbsp.extend(writer.finish());

    let sps = Sps::parse(&rbsp).unwrap();
    assert_eq!(sps.log2_max_frame_num, 6);
    assert_eq!(sps.pic_order_cnt_type, 0);
    assert_eq!(sps.log2_max_pic_order_cnt_lsb, 7);
    assert_eq!(sps.pic_width_in_mbs, 1);
    assert_eq!(sps.pic_height_in_mbs, 1);
}

#[test]
fn parse_applies_frame_cropping_rectangle() {
    // 1 macroblock (16x16) raw, cropped by 1 crop-unit on left and top (4:2:0 => crop unit 2px).
    let rbsp = build_baseline_sps_rbsp(66, 30, 0, 0, Some((1, 0, 1, 0)));
    let sps = Sps::parse(&rbsp).unwrap();
    assert_eq!(sps.width, 16 - 2);
    assert_eq!(sps.height, 16 - 2);
}

#[test]
fn parse_high_profile_skips_chroma_and_bit_depth_fields() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // seq_parameter_set_id
    writer.write_ue(1); // chroma_format_idc = 4:2:0
    writer.write_ue(0); // bit_depth_luma_minus8
    writer.write_ue(0); // bit_depth_chroma_minus8
    writer.push_bit(0); // qpprime_y_zero_transform_bypass_flag
    writer.push_bit(0); // seq_scaling_matrix_present_flag = false
    writer.write_ue(0); // log2_max_frame_num_minus4
    writer.write_ue(0); // pic_order_cnt_type
    writer.write_ue(0); // log2_max_pic_order_cnt_lsb_minus4
    writer.write_ue(1); // max_num_ref_frames
    writer.push_bit(0); // gaps_in_frame_num_value_allowed_flag
    writer.write_ue(9); // pic_width_in_mbs_minus1 (10*16=160)
    writer.write_ue(9); // pic_height_in_map_units_minus1 (10*16=160)
    writer.push_bit(1); // frame_mbs_only_flag
    writer.push_bit(1); // direct_8x8_inference_flag
    writer.push_bit(0); // frame_cropping_flag

    let mut rbsp = vec![100u8, 0, 40]; // High profile, level 4.0
    rbsp.extend(writer.finish());

    let sps = Sps::parse(&rbsp).unwrap();
    assert_eq!(sps.profile_idc, 100);
    assert_eq!(sps.width, 160);
    assert_eq!(sps.height, 160);
}

#[test]
fn parse_high_profile_with_scaling_list_stops_early_and_stays_bit_aligned() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // seq_parameter_set_id
    writer.write_ue(1); // chroma_format_idc = 4:2:0 -> 8 scaling lists
    writer.write_ue(0); // bit_depth_luma_minus8
    writer.write_ue(0); // bit_depth_chroma_minus8
    writer.push_bit(0); // qpprime_y_zero_transform_bypass_flag
    writer.push_bit(1); // seq_scaling_matrix_present_flag = true
    // List 0: present, with an early stop (next_scale reaches 0 after 2 deltas of size 16).
    writer.push_bit(1); // seq_scaling_list_present_flag[0]
    writer.write_se(0); // delta_scale -> next_scale = 8
    writer.write_se(-8); // delta_scale -> next_scale = (8 - 8 + 256) % 256 = 0, stop early
    // Lists 1..7: not present.
    for _ in 1..8 {
        writer.push_bit(0);
    }
    writer.write_ue(0); // log2_max_frame_num_minus4
    writer.write_ue(0); // pic_order_cnt_type
    writer.write_ue(0); // log2_max_pic_order_cnt_lsb_minus4
    writer.write_ue(1); // max_num_ref_frames
    writer.push_bit(0); // gaps_in_frame_num_value_allowed_flag
    writer.write_ue(4); // pic_width_in_mbs_minus1 (5*16=80)
    writer.write_ue(4); // pic_height_in_map_units_minus1 (5*16=80)
    writer.push_bit(1); // frame_mbs_only_flag
    writer.push_bit(1); // direct_8x8_inference_flag
    writer.push_bit(0); // frame_cropping_flag

    let mut rbsp = vec![100u8, 0, 40];
    rbsp.extend(writer.finish());

    let sps = Sps::parse(&rbsp).unwrap();
    assert_eq!(sps.width, 80);
    assert_eq!(sps.height, 80);
}

#[test]
fn parse_errors_on_truncated_input_with_no_body_bits() {
    let rbsp = [66u8, 0, 30]; // profile/constraints/level only, no body bits at all
    assert_eq!(Sps::parse(&rbsp), Err(H264Error::UnexpectedEof));
}

#[test]
fn parse_errors_on_truncated_input_missing_header_bytes() {
    let rbsp = [66u8]; // missing constraint byte and level_idc
    assert_eq!(Sps::parse(&rbsp), Err(H264Error::UnexpectedEof));
}

#[test]
fn parse_errors_when_chroma_format_idc_exceeds_valid_range() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // seq_parameter_set_id
    writer.write_ue(4); // chroma_format_idc = 4 (spec range is 0..=3)
    writer.write_ue(0); // bit_depth_luma_minus8
    writer.write_ue(0); // bit_depth_chroma_minus8
    writer.push_bit(0); // qpprime_y_zero_transform_bypass_flag
    writer.push_bit(0); // seq_scaling_matrix_present_flag = false

    let mut rbsp = vec![100u8, 0, 40];
    rbsp.extend(writer.finish());

    assert_eq!(Sps::parse(&rbsp), Err(H264Error::InvalidChromaFormat));
}
