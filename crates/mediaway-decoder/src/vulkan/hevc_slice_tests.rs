#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal MSB-first bit packer — mirrors `hevc_params_tests.rs`'s own
/// `BitWriter` (duplicated here to keep this test file self-contained, per
/// this crate's existing test-file convention).
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

    fn finish(mut self) -> Vec<u8> {
        while self.nbits != 0 {
            self.push_bit(0);
        }
        self.bytes
    }
}

fn make_sps(log2_max_pic_order_cnt_lsb: u32) -> HevcSps {
    HevcSps {
        sps_video_parameter_set_id: 0,
        sps_seq_parameter_set_id: 0,
        pic_width_in_luma_samples: 64,
        pic_height_in_luma_samples: 16,
        log2_max_pic_order_cnt_lsb,
        max_dec_pic_buffering: 2,
        log2_min_cb_size: 3,
        log2_diff_max_min_cb_size: 2,
        log2_min_tb_size: 2,
        log2_diff_max_min_tb_size: 3,
        max_transform_hierarchy_depth_inter: 0,
        max_transform_hierarchy_depth_intra: 0,
        general_profile_idc: 1,
        general_level_idc: 60,
        general_tier_flag: false,
        general_progressive_source_flag: true,
        general_interlaced_source_flag: false,
        general_non_packed_constraint_flag: true,
        general_frame_only_constraint_flag: true,
        amp_enabled_flag: false,
        sample_adaptive_offset_enabled_flag: false,
        sps_temporal_mvp_enabled_flag: false,
        strong_intra_smoothing_enabled_flag: false,
    }
}

fn make_pps(num_extra_slice_header_bits: u32, output_flag_present_flag: bool) -> HevcPps {
    HevcPps {
        pps_pic_parameter_set_id: 0,
        pps_seq_parameter_set_id: 0,
        output_flag_present_flag,
        num_extra_slice_header_bits,
        num_ref_idx_l0_default_active: 1,
        num_ref_idx_l1_default_active: 1,
        init_qp: 26,
        dependent_slice_segments_enabled_flag: false,
        sign_data_hiding_enabled_flag: false,
        cabac_init_present_flag: false,
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
    }
}

#[test]
fn parse_idr_i_slice_has_no_poc_or_rps() {
    let sps = make_sps(8);
    let pps = make_pps(0, false);
    let mut writer = BitWriter::new();
    writer.push_bit(1); // first_slice_segment_in_pic_flag
    writer.push_bit(0); // no_output_of_prior_pics_flag (IDR)
    writer.write_ue(0); // slice_pic_parameter_set_id
    writer.write_ue(2); // slice_type = I
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let header =
        HevcSliceSegmentHeader::parse(&mut reader, &sps, &pps, HevcNalUnitType::Idr).unwrap();
    assert!(matches!(header.slice_type, HevcSliceType::I));
    assert_eq!(header.slice_pic_parameter_set_id, 0);
    assert!(header.pic_order_cnt_lsb.is_none());
    assert!(header.short_term_rps.is_none());
}

#[test]
fn parse_non_idr_slice_reads_poc_lsb_and_short_term_rps() {
    let sps = make_sps(8);
    let pps = make_pps(0, false);
    let mut writer = BitWriter::new();
    writer.push_bit(1); // first_slice_segment_in_pic_flag
    // nal_unit_type = Trail: not IDR/CRA, so no no_output_of_prior_pics_flag bit.
    writer.write_ue(0); // slice_pic_parameter_set_id
    writer.write_ue(1); // slice_type = P
    writer.write_bits(5, 8); // slice_pic_order_cnt_lsb = 5 (log2_max_pic_order_cnt_lsb = 8)
    writer.push_bit(0); // short_term_ref_pic_set_sps_flag = 0
    // short_term_ref_pic_set(0):
    writer.write_ue(1); // num_negative_pics = 1
    writer.write_ue(0); // num_positive_pics = 0
    writer.write_ue(0); // delta_poc_s0_minus1[0] = 0 -> DeltaPocS0[0] = -1
    writer.push_bit(1); // used_by_curr_pic_s0[0] = 1
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let header =
        HevcSliceSegmentHeader::parse(&mut reader, &sps, &pps, HevcNalUnitType::Trail).unwrap();
    assert!(matches!(header.slice_type, HevcSliceType::P));
    assert_eq!(header.pic_order_cnt_lsb, Some(5));
    let rps = header.short_term_rps.unwrap();
    assert_eq!(rps.s0.len(), 1);
    assert_eq!(rps.s0[0].delta_poc, -1);
    assert!(rps.s0[0].used_by_curr_pic);
    assert!(rps.s1.is_empty());

    let (before, after) = rps.curr_before_after_poc(10);
    assert_eq!(before.as_slice(), &[9]);
    assert!(after.is_empty());
}

#[test]
fn parse_cra_slice_still_reads_poc_and_rps() {
    // CRA is an intra random-access point but, unlike IDR, is NOT exempt from
    // POC-LSB/RPS signaling (ITU-T H.265 § 7.3.6.1) — this was a real mistake
    // caught while first writing `HevcSliceSegmentHeader::parse` (see
    // `hevc_params.rs`'s `HevcNalUnitType::is_idr` doc comment).
    let sps = make_sps(8);
    let pps = make_pps(0, false);
    let mut writer = BitWriter::new();
    writer.push_bit(1); // first_slice_segment_in_pic_flag
    writer.push_bit(0); // no_output_of_prior_pics_flag (CRA also reads this)
    writer.write_ue(0); // slice_pic_parameter_set_id
    writer.write_ue(2); // slice_type = I
    writer.write_bits(0, 8); // slice_pic_order_cnt_lsb = 0
    writer.push_bit(0); // short_term_ref_pic_set_sps_flag = 0
    writer.write_ue(0); // num_negative_pics = 0
    writer.write_ue(0); // num_positive_pics = 0
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let header =
        HevcSliceSegmentHeader::parse(&mut reader, &sps, &pps, HevcNalUnitType::Cra).unwrap();
    assert_eq!(header.pic_order_cnt_lsb, Some(0));
    let rps = header.short_term_rps.unwrap();
    assert!(rps.s0.is_empty());
    assert!(rps.s1.is_empty());
}

#[test]
fn parse_rejects_b_slice() {
    let sps = make_sps(8);
    let pps = make_pps(0, false);
    let mut writer = BitWriter::new();
    writer.push_bit(1); // first_slice_segment_in_pic_flag
    writer.write_ue(0); // slice_pic_parameter_set_id
    writer.write_ue(0); // slice_type = B
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let err =
        HevcSliceSegmentHeader::parse(&mut reader, &sps, &pps, HevcNalUnitType::Trail).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn parse_rejects_non_first_slice_segment() {
    let sps = make_sps(8);
    let pps = make_pps(0, false);
    let mut writer = BitWriter::new();
    writer.push_bit(0); // first_slice_segment_in_pic_flag = 0 (unsupported)
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let err =
        HevcSliceSegmentHeader::parse(&mut reader, &sps, &pps, HevcNalUnitType::Trail).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn parse_rejects_sps_level_rps_reference() {
    let sps = make_sps(8);
    let pps = make_pps(0, false);
    let mut writer = BitWriter::new();
    writer.push_bit(1); // first_slice_segment_in_pic_flag
    writer.write_ue(0); // slice_pic_parameter_set_id
    writer.write_ue(1); // slice_type = P
    writer.write_bits(0, 8); // slice_pic_order_cnt_lsb
    writer.push_bit(1); // short_term_ref_pic_set_sps_flag = 1 (unsupported)
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let err =
        HevcSliceSegmentHeader::parse(&mut reader, &sps, &pps, HevcNalUnitType::Trail).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn parse_skips_extra_slice_header_bits_and_pic_output_flag() {
    let sps = make_sps(8);
    let pps = make_pps(3, true); // num_extra_slice_header_bits = 3, output_flag_present_flag = true
    let mut writer = BitWriter::new();
    writer.push_bit(1); // first_slice_segment_in_pic_flag
    writer.push_bit(0); // no_output_of_prior_pics_flag (IDR)
    writer.write_ue(0); // slice_pic_parameter_set_id
    writer.write_bits(0b101, 3); // slice_reserved_flag[0..3]
    writer.write_ue(2); // slice_type = I
    writer.push_bit(1); // pic_output_flag
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let header =
        HevcSliceSegmentHeader::parse(&mut reader, &sps, &pps, HevcNalUnitType::Idr).unwrap();
    assert!(matches!(header.slice_type, HevcSliceType::I));
}

#[test]
fn short_term_rps_parse_rejects_too_many_negative_pics() {
    let mut writer = BitWriter::new();
    writer.write_ue(9); // num_negative_pics = 9 (> 8-entry StdVideoDecodeH265PictureInfo capacity)
    writer.write_ue(0); // num_positive_pics
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let err = ShortTermRefPicSet::parse(&mut reader).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn short_term_rps_parse_accumulates_positive_deltas() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // num_negative_pics = 0
    writer.write_ue(2); // num_positive_pics = 2
    writer.write_ue(0); // delta_poc_s1_minus1[0] = 0 -> DeltaPocS1[0] = 1
    writer.push_bit(1); // used_by_curr_pic_s1[0] = 1
    writer.write_ue(1); // delta_poc_s1_minus1[1] = 1 -> DeltaPocS1[1] = 1 + 2 = 3
    writer.push_bit(0); // used_by_curr_pic_s1[1] = 0
    let rbsp = writer.finish();
    let mut reader = BitReader::new(&rbsp);
    let rps = ShortTermRefPicSet::parse(&mut reader).unwrap();
    assert!(rps.s0.is_empty());
    assert_eq!(rps.s1.len(), 2);
    assert_eq!(rps.s1[0].delta_poc, 1);
    assert!(rps.s1[0].used_by_curr_pic);
    assert_eq!(rps.s1[1].delta_poc, 3);
    assert!(!rps.s1[1].used_by_curr_pic);

    let (before, after) = rps.curr_before_after_poc(10);
    assert!(before.is_empty());
    assert_eq!(after.as_slice(), &[11]);
}
