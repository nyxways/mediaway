#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use crate::vulkan::dpb::DpbSlot;

/// Minimal MSB-first bit packer — same convention as `h264_params_tests.rs`'s
/// own `BitWriter` (duplicated here to keep this test file self-contained).
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

/// `profile_tier_level(profilePresentFlag=1, maxNumSubLayersMinus1=0)`
/// (96 bits total) with the given `general_profile_idc`/`general_level_idc`,
/// everything else zeroed.
fn write_profile_tier_level(writer: &mut BitWriter, profile_idc: u32, level_idc: u32) {
    writer.write_bits(0, 2); // general_profile_space
    writer.push_bit(0); // general_tier_flag
    writer.write_bits(profile_idc, 5); // general_profile_idc
    writer.write_bits(0, 32); // general_profile_compatibility_flag[32]
    writer.push_bit(1); // general_progressive_source_flag
    writer.push_bit(0); // general_interlaced_source_flag
    writer.push_bit(1); // general_non_packed_constraint_flag
    writer.push_bit(1); // general_frame_only_constraint_flag
    writer.write_bits(0, 32); // reserved (part 1 of 44)
    writer.write_bits(0, 12); // reserved (part 2 of 44)
    writer.write_bits(level_idc, 8); // general_level_idc
}

fn build_sps_rbsp(max_num_ref_frames_minus1: u32, width: u32, height: u32) -> Vec<u8> {
    let mut writer = BitWriter::new();
    writer.write_bits(0, 4); // sps_video_parameter_set_id
    writer.write_bits(0, 3); // sps_max_sub_layers_minus1
    writer.push_bit(1); // sps_temporal_id_nesting_flag
    write_profile_tier_level(&mut writer, 1, 60); // Main profile, level 2.0
    writer.write_ue(0); // sps_seq_parameter_set_id
    writer.write_ue(1); // chroma_format_idc = 4:2:0
    writer.write_ue(width); // pic_width_in_luma_samples
    writer.write_ue(height); // pic_height_in_luma_samples
    writer.push_bit(0); // conformance_window_flag
    writer.write_ue(0); // bit_depth_luma_minus8
    writer.write_ue(0); // bit_depth_chroma_minus8
    writer.write_ue(0); // log2_max_pic_order_cnt_lsb_minus4
    writer.push_bit(1); // sps_sub_layer_ordering_info_present_flag
    writer.write_ue(max_num_ref_frames_minus1); // sps_max_dec_pic_buffering_minus1[0]
    writer.write_ue(0); // sps_max_num_reorder_pics[0]
    writer.write_ue(0); // sps_max_latency_increase_plus1[0]
    writer.write_ue(0); // log2_min_luma_coding_block_size_minus3
    writer.write_ue(2); // log2_diff_max_min_luma_coding_block_size
    writer.write_ue(0); // log2_min_luma_transform_block_size_minus2
    writer.write_ue(3); // log2_diff_max_min_luma_transform_block_size
    writer.write_ue(3); // max_transform_hierarchy_depth_inter
    writer.write_ue(3); // max_transform_hierarchy_depth_intra
    writer.push_bit(0); // scaling_list_enabled_flag
    writer.push_bit(1); // amp_enabled_flag
    writer.push_bit(1); // sample_adaptive_offset_enabled_flag
    writer.push_bit(0); // pcm_enabled_flag
    writer.write_ue(0); // num_short_term_ref_pic_sets
    writer.push_bit(0); // long_term_ref_pics_present_flag
    writer.push_bit(1); // sps_temporal_mvp_enabled_flag
    writer.push_bit(0); // strong_intra_smoothing_enabled_flag
    writer.finish()
}

#[test]
fn sps_parse_extracts_expected_fields() {
    let rbsp = build_sps_rbsp(1, 64, 16);
    let sps = HevcSps::parse(&rbsp).unwrap();
    assert_eq!(sps.pic_width_in_luma_samples, 64);
    assert_eq!(sps.pic_height_in_luma_samples, 16);
    assert_eq!(sps.log2_max_pic_order_cnt_lsb, 4);
    assert_eq!(sps.max_dec_pic_buffering, 2);
    assert_eq!(sps.log2_min_cb_size, 3);
    assert_eq!(sps.log2_diff_max_min_cb_size, 2);
    assert_eq!(sps.log2_min_tb_size, 2);
    assert_eq!(sps.log2_diff_max_min_tb_size, 3);
    assert_eq!(sps.general_profile_idc, 1);
    assert_eq!(sps.general_level_idc, 60);
    // `write_profile_tier_level` signals general_tier_flag=0,
    // progressive_source=1, interlaced_source=0, non_packed_constraint=1,
    // frame_only_constraint=1 — must be echoed exactly, not hardcoded (the
    // real hardware bug `to_std_profile_tier_level`'s doc describes).
    assert!(!sps.general_tier_flag);
    assert!(sps.general_progressive_source_flag);
    assert!(!sps.general_interlaced_source_flag);
    assert!(sps.general_non_packed_constraint_flag);
    assert!(sps.general_frame_only_constraint_flag);
    // These four must be echoed exactly, not silently zeroed — the real
    // hardware bug this crate's own HEVC decode hit (see `HevcSps::to_std`'s
    // doc). Set to a mix of true/false here so a regression back to
    // "always false" would be caught.
    assert!(sps.amp_enabled_flag);
    assert!(sps.sample_adaptive_offset_enabled_flag);
    assert!(sps.sps_temporal_mvp_enabled_flag);
    assert!(!sps.strong_intra_smoothing_enabled_flag);
}

#[test]
fn to_std_profile_tier_level_converts_raw_level_idc_to_vulkan_ordinal() {
    // Level 2.0's raw ITU-T H.265 general_level_idc is 60 (30 * 2.0), but
    // StdVideoH265LevelIdc's ordinal for Level 2.0 is 1
    // (STD_VIDEO_H265_LEVEL_IDC_2_0) — a direct `i32::from(60)` cast (this
    // crate's original bug) would build an out-of-range enum value with no
    // defined meaning to the driver.
    let rbsp = build_sps_rbsp(1, 64, 16);
    let sps = HevcSps::parse(&rbsp).unwrap();
    let ptl = sps.to_std_profile_tier_level();
    assert_eq!(ptl.general_level_idc, native::STD_VIDEO_H265_LEVEL_IDC_2_0);
    assert_eq!(
        ptl.general_profile_idc,
        native::STD_VIDEO_H265_PROFILE_IDC_MAIN
    );
    assert_eq!(ptl.flags.general_progressive_source_flag(), 1);
    assert_eq!(ptl.flags.general_frame_only_constraint_flag(), 1);
    assert_eq!(ptl.flags.general_tier_flag(), 0);
    assert_eq!(ptl.flags.general_interlaced_source_flag(), 0);
    assert_eq!(ptl.flags.general_non_packed_constraint_flag(), 1);
}

#[test]
fn sps_parse_rejects_multiple_sub_layers() {
    let mut writer = BitWriter::new();
    writer.write_bits(0, 4); // sps_video_parameter_set_id
    writer.write_bits(1, 3); // sps_max_sub_layers_minus1 = 1 (unsupported)
    let rbsp = writer.finish();
    let err = HevcSps::parse(&rbsp).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn sps_parse_rejects_nonzero_short_term_rps_count() {
    let mut writer = BitWriter::new();
    writer.write_bits(0, 4);
    writer.write_bits(0, 3);
    writer.push_bit(1);
    write_profile_tier_level(&mut writer, 1, 60);
    writer.write_ue(0); // sps_seq_parameter_set_id
    writer.write_ue(1); // chroma_format_idc
    writer.write_ue(64); // width
    writer.write_ue(16); // height
    writer.push_bit(0); // conformance_window_flag
    writer.write_ue(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.push_bit(1);
    writer.write_ue(1);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.write_ue(2);
    writer.write_ue(0);
    writer.write_ue(3);
    writer.write_ue(3);
    writer.write_ue(3);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.write_ue(1); // num_short_term_ref_pic_sets = 1 (unsupported)
    let rbsp = writer.finish();
    let err = HevcSps::parse(&rbsp).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn pps_parse_extracts_expected_fields() {
    // Build using write_ue/se helpers matching H264Pps test convention.
    let mut writer = BitWriter::new();
    writer.write_ue(0); // pps_pic_parameter_set_id
    writer.write_ue(0); // pps_seq_parameter_set_id
    writer.push_bit(0); // dependent_slice_segments_enabled_flag
    writer.push_bit(1); // output_flag_present_flag
    writer.write_bits(2, 3); // num_extra_slice_header_bits = 2
    writer.push_bit(0); // sign_data_hiding_enabled_flag
    writer.push_bit(0); // cabac_init_present_flag
    writer.write_ue(0); // num_ref_idx_l0_default_active_minus1
    writer.write_ue(0); // num_ref_idx_l1_default_active_minus1
    writer.write_ue(0); // init_qp_minus26 (se encoded 0 -> ue(0))
    writer.push_bit(0); // constrained_intra_pred_flag
    writer.push_bit(0); // transform_skip_enabled_flag
    writer.push_bit(0); // cu_qp_delta_enabled_flag
    writer.write_ue(0); // pps_cb_qp_offset (se -> ue(0))
    writer.write_ue(0); // pps_cr_qp_offset
    writer.push_bit(0); // pps_slice_chroma_qp_offsets_present_flag
    writer.push_bit(0); // weighted_pred_flag
    writer.push_bit(0); // weighted_bipred_flag
    writer.push_bit(0); // transquant_bypass_enabled_flag
    writer.push_bit(0); // tiles_enabled_flag
    writer.push_bit(0); // entropy_coding_sync_enabled_flag
    writer.push_bit(1); // pps_loop_filter_across_slices_enabled_flag
    writer.push_bit(0); // deblocking_filter_control_present_flag
    let rbsp = writer.finish();

    let pps = HevcPps::parse(&rbsp).unwrap();
    assert!(pps.output_flag_present_flag);
    assert_eq!(pps.num_extra_slice_header_bits, 2);
    assert_eq!(pps.num_ref_idx_l0_default_active, 1);
    assert_eq!(pps.init_qp, 26);
    // Must be echoed exactly, not left always-false — the real hardware bug
    // this field's own doc describes (see `HevcPps::pps_loop_filter_across_slices_enabled_flag`).
    assert!(pps.pps_loop_filter_across_slices_enabled_flag);
}

#[test]
fn pps_parse_rejects_deblocking_filter_control_present() {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // pps_pic_parameter_set_id
    writer.write_ue(0); // pps_seq_parameter_set_id
    writer.push_bit(0); // dependent_slice_segments_enabled_flag
    writer.push_bit(0); // output_flag_present_flag
    writer.write_bits(0, 3); // num_extra_slice_header_bits
    writer.push_bit(0); // sign_data_hiding_enabled_flag
    writer.push_bit(0); // cabac_init_present_flag
    writer.write_ue(0); // num_ref_idx_l0_default_active_minus1
    writer.write_ue(0); // num_ref_idx_l1_default_active_minus1
    writer.write_ue(0); // init_qp_minus26
    writer.push_bit(0); // constrained_intra_pred_flag
    writer.push_bit(0); // transform_skip_enabled_flag
    writer.push_bit(0); // cu_qp_delta_enabled_flag
    writer.write_ue(0); // pps_cb_qp_offset
    writer.write_ue(0); // pps_cr_qp_offset
    writer.push_bit(0); // pps_slice_chroma_qp_offsets_present_flag
    writer.push_bit(0); // weighted_pred_flag
    writer.push_bit(0); // weighted_bipred_flag
    writer.push_bit(0); // transquant_bypass_enabled_flag
    writer.push_bit(0); // tiles_enabled_flag
    writer.push_bit(0); // entropy_coding_sync_enabled_flag
    writer.push_bit(0); // pps_loop_filter_across_slices_enabled_flag
    writer.push_bit(1); // deblocking_filter_control_present_flag = 1 (unsupported)
    let rbsp = writer.finish();
    let err = HevcPps::parse(&rbsp).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn pps_parse_rejects_tiles_enabled() {
    let mut writer = BitWriter::new();
    writer.write_ue(0);
    writer.write_ue(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.write_bits(0, 3);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(1); // tiles_enabled_flag = 1 (unsupported)
    let rbsp = writer.finish();
    let err = HevcPps::parse(&rbsp).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn pps_parse_rejects_wpp_entropy_coding_sync() {
    let mut writer = BitWriter::new();
    writer.write_ue(0);
    writer.write_ue(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.write_bits(0, 3);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.write_ue(0);
    writer.write_ue(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(0);
    writer.push_bit(0); // tiles_enabled_flag = 0
    writer.push_bit(1); // entropy_coding_sync_enabled_flag = 1 (unsupported)
    let rbsp = writer.finish();
    let err = HevcPps::parse(&rbsp).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn vps_parse_extracts_id() {
    let rbsp = [0x30u8, 0x00]; // vps_video_parameter_set_id = 3 in top 4 bits
    let vps = HevcVps::parse(&rbsp).unwrap();
    assert_eq!(vps.vps_video_parameter_set_id, 3);
}

#[test]
fn nal_unit_parse_extracts_header_fields() {
    // nal_unit_type = 33 (SPS), nuh_layer_id = 0, nuh_temporal_id_plus1 = 1.
    let first = (33u8) << 1;
    let second = 0x1u8;
    let data = [first, second, 0xAB, 0xCD];
    let nal = HevcNalUnit::parse(&data).unwrap();
    assert!(matches!(nal.unit_type, HevcNalUnitType::Sps));
    assert_eq!(nal.nuh_layer_id, 0);
    assert_eq!(nal.temporal_id, 0);
    assert_eq!(nal.rbsp, vec![0xAB, 0xCD]);
}

#[test]
fn nal_unit_parse_rejects_nonzero_layer_id() {
    let first = (33u8) << 1 | 0x1; // nuh_layer_id high bit set
    let second = 0x1u8;
    let data = [first, second];
    let err = HevcNalUnit::parse(&data).unwrap_err();
    assert!(matches!(err, HevcParamError::Unsupported { .. }));
}

#[test]
fn reference_info_from_slot_carries_poc_and_reference_flag() {
    let slot = DpbSlot::new_reference(0, 0, 7);
    let info = reference_info_from_slot(&slot);
    assert_eq!(info.PicOrderCntVal, 7);
    // `new_reference` always sets `used_for_reference = true`, so the
    // "unused for reference" flag must be clear.
    assert_eq!(info.flags.unused_for_reference(), 0);
    assert_eq!(info.flags.used_for_long_term_reference(), 0);
}
