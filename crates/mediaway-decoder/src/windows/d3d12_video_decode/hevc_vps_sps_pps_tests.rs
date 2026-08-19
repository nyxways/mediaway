//! Pure unit tests for [`super::parse_sps`]/[`super::parse_pps`] against hand-built RBSP
//! bitstreams (same tiny Exp-Golomb bit writer approach as `h264_sps_pps_tests.rs`,
//! test-only, duplicated rather than shared across sibling test files).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{parse_pps, parse_sps};

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

/// `profile_tier_level(1, 0)` is exactly 96 bits (12 bytes): 2+1+5 (profile
/// space/tier/idc) + 32 (compatibility flags) + 48 (source/constraint + reserved bits,
/// opaque to this crate's `skip_profile_tier_level`) + 8 (`general_level_idc`). Content
/// doesn't matter — `parse_sps` only skips these bits, never reads their values.
fn write_ptl_zeros(w: &mut BitWriter) {
    w.write_bits(0, 2 + 1 + 5); // profile_space + tier_flag + profile_idc
    w.write_bits(0, 32); // profile_compatibility_flags
    w.write_bits(0, 32);
    w.write_bits(0, 16); // (32 + 16 == 48 constraint/reserved bits)
    w.write_bits(0, 8); // general_level_idc
}

#[allow(
    clippy::too_many_arguments,
    reason = "one linear SPS field fixture builder"
)]
fn sps_bytes(
    chroma_format_idc: u32,
    bit_depth_luma_minus8: u32,
    bit_depth_chroma_minus8: u32,
    scaling_list_enabled_flag: bool,
    pcm_enabled_flag: bool,
    num_short_term_ref_pic_sets: u32,
    long_term_ref_pics_present_flag: bool,
    sps_max_sub_layers_minus1: u32,
) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.write_bits(0, 4); // sps_video_parameter_set_id
    w.write_bits(sps_max_sub_layers_minus1, 3);
    w.write_bit(false); // sps_temporal_id_nesting_flag
    write_ptl_zeros(&mut w);
    w.write_ue(0); // sps_seq_parameter_set_id
    w.write_ue(chroma_format_idc);
    if chroma_format_idc == 3 {
        w.write_bit(false); // separate_colour_plane_flag
    }
    w.write_ue(352); // pic_width_in_luma_samples
    w.write_ue(288); // pic_height_in_luma_samples
    w.write_bit(false); // conformance_window_flag
    w.write_ue(bit_depth_luma_minus8);
    w.write_ue(bit_depth_chroma_minus8);
    w.write_ue(4); // log2_max_pic_order_cnt_lsb_minus4 -> 8
    w.write_bit(false); // sps_sub_layer_ordering_info_present_flag
    // one ordering triple always read since sps_max_sub_layers_minus1 is forced 0 for
    // every test that expects to reach further fields (the sub-layer rejection test
    // stops before this point anyway).
    w.write_ue(3); // sps_max_dec_pic_buffering_minus1 -> 4
    w.write_ue(0); // sps_max_num_reorder_pics
    w.write_ue(0); // sps_max_latency_increase_plus1
    w.write_ue(3); // log2_min_luma_coding_block_size_minus3 -> 6
    w.write_ue(2); // log2_diff_max_min_luma_coding_block_size
    w.write_ue(0); // log2_min_luma_transform_block_size_minus2 -> 2
    w.write_ue(3); // log2_diff_max_min_luma_transform_block_size
    w.write_ue(2); // max_transform_hierarchy_depth_inter
    w.write_ue(1); // max_transform_hierarchy_depth_intra
    w.write_bit(scaling_list_enabled_flag);
    if !scaling_list_enabled_flag {
        w.write_bit(true); // amp_enabled_flag
        w.write_bit(true); // sample_adaptive_offset_enabled_flag
        w.write_bit(pcm_enabled_flag);
        if !pcm_enabled_flag {
            w.write_ue(num_short_term_ref_pic_sets);
            if num_short_term_ref_pic_sets == 0 {
                w.write_bit(long_term_ref_pics_present_flag);
                if !long_term_ref_pics_present_flag {
                    w.write_bit(true); // sps_temporal_mvp_enabled_flag
                    w.write_bit(true); // strong_intra_smoothing_enabled_flag
                    w.write_bit(false); // vui_parameters_present_flag
                }
            }
        }
    }
    w.finish()
}

fn valid_sps_bytes() -> Vec<u8> {
    sps_bytes(1, 0, 0, false, false, 0, false, 0)
}

#[test]
fn parse_sps_roundtrips_fields() {
    let sps = parse_sps(&valid_sps_bytes()).expect("valid hand-built SPS");
    assert_eq!(sps.pic_width_in_luma_samples, 352);
    assert_eq!(sps.pic_height_in_luma_samples, 288);
    assert_eq!(sps.log2_max_pic_order_cnt_lsb, 8);
    assert_eq!(sps.max_dec_pic_buffering, 4);
    assert_eq!(sps.log2_min_cb_size, 6);
    assert_eq!(sps.log2_diff_max_min_cb_size, 2);
    assert_eq!(sps.log2_min_tb_size, 2);
    assert_eq!(sps.log2_diff_max_min_tb_size, 3);
    assert_eq!(sps.max_transform_hierarchy_depth_inter, 2);
    assert_eq!(sps.max_transform_hierarchy_depth_intra, 1);
    assert!(sps.amp_enabled_flag);
    assert!(sps.sample_adaptive_offset_enabled_flag);
    assert!(sps.sps_temporal_mvp_enabled_flag);
    assert!(sps.strong_intra_smoothing_enabled_flag);
}

#[test]
fn parse_sps_rejects_non_420_chroma() {
    let bytes = sps_bytes(2, 0, 0, false, false, 0, false, 0);
    assert!(parse_sps(&bytes).is_err());
}

#[test]
fn parse_sps_rejects_nonzero_bit_depth() {
    assert!(parse_sps(&sps_bytes(1, 1, 0, false, false, 0, false, 0)).is_err());
    assert!(parse_sps(&sps_bytes(1, 0, 1, false, false, 0, false, 0)).is_err());
}

#[test]
fn parse_sps_rejects_scaling_list_enabled() {
    let bytes = sps_bytes(1, 0, 0, true, false, 0, false, 0);
    assert!(parse_sps(&bytes).is_err());
}

#[test]
fn parse_sps_rejects_pcm_enabled() {
    let bytes = sps_bytes(1, 0, 0, false, true, 0, false, 0);
    assert!(parse_sps(&bytes).is_err());
}

#[test]
fn parse_sps_rejects_sps_level_rps_list() {
    let bytes = sps_bytes(1, 0, 0, false, false, 1, false, 0);
    assert!(parse_sps(&bytes).is_err());
}

#[test]
fn parse_sps_rejects_long_term_ref_pics_present() {
    let bytes = sps_bytes(1, 0, 0, false, false, 0, true, 0);
    assert!(parse_sps(&bytes).is_err());
}

#[test]
fn parse_sps_rejects_multiple_sub_layers() {
    let bytes = sps_bytes(1, 0, 0, false, false, 0, false, 1);
    assert!(parse_sps(&bytes).is_err());
}

#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "one linear PPS field fixture builder, test-only"
)]
fn pps_bytes(
    tiles_enabled_flag: bool,
    entropy_coding_sync_enabled_flag: bool,
    deblocking_filter_control_present_flag: bool,
    pps_scaling_list_data_present_flag: bool,
    lists_modification_present_flag: bool,
    log2_parallel_merge_level_minus2: u32,
) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.write_ue(0); // pps_pic_parameter_set_id
    w.write_ue(0); // pps_seq_parameter_set_id
    w.write_bit(false); // dependent_slice_segments_enabled_flag
    w.write_bit(false); // output_flag_present_flag
    w.write_bits(0, 3); // num_extra_slice_header_bits
    w.write_bit(false); // sign_data_hiding_enabled_flag
    w.write_bit(false); // cabac_init_present_flag
    w.write_ue(0); // num_ref_idx_l0_default_active_minus1
    w.write_ue(0); // num_ref_idx_l1_default_active_minus1
    w.write_se(0); // init_qp_minus26
    w.write_bit(false); // constrained_intra_pred_flag
    w.write_bit(false); // transform_skip_enabled_flag
    w.write_bit(false); // cu_qp_delta_enabled_flag (no diff_cu_qp_delta_depth follows)
    w.write_se(0); // pps_cb_qp_offset
    w.write_se(0); // pps_cr_qp_offset
    w.write_bit(false); // pps_slice_chroma_qp_offsets_present_flag
    w.write_bit(false); // weighted_pred_flag
    w.write_bit(false); // weighted_bipred_flag
    w.write_bit(false); // transquant_bypass_enabled_flag
    w.write_bit(tiles_enabled_flag);
    w.write_bit(entropy_coding_sync_enabled_flag);
    if !tiles_enabled_flag && !entropy_coding_sync_enabled_flag {
        w.write_bit(false); // pps_loop_filter_across_slices_enabled_flag
        w.write_bit(deblocking_filter_control_present_flag);
        if !deblocking_filter_control_present_flag {
            w.write_bit(pps_scaling_list_data_present_flag);
            if !pps_scaling_list_data_present_flag {
                w.write_bit(lists_modification_present_flag);
                w.write_ue(log2_parallel_merge_level_minus2);
                w.write_bit(false); // slice_segment_header_extension_present_flag
                w.write_bit(false); // pps_extension_present_flag
            }
        }
    }
    w.finish()
}

fn valid_pps_bytes() -> Vec<u8> {
    pps_bytes(false, false, false, false, false, 1)
}

#[test]
fn parse_pps_roundtrips_fields() {
    let pps = parse_pps(&valid_pps_bytes()).expect("valid hand-built PPS");
    assert_eq!(pps.log2_parallel_merge_level_minus2, 1);
    assert!(!pps.lists_modification_present_flag);
    assert!(!pps.slice_segment_header_extension_present_flag);
}

#[test]
fn parse_pps_rejects_tiles_enabled() {
    let bytes = pps_bytes(true, false, false, false, false, 0);
    assert!(parse_pps(&bytes).is_err());
}

#[test]
fn parse_pps_rejects_wpp() {
    let bytes = pps_bytes(false, true, false, false, false, 0);
    assert!(parse_pps(&bytes).is_err());
}

#[test]
fn parse_pps_rejects_deblocking_filter_control_present() {
    let bytes = pps_bytes(false, false, true, false, false, 0);
    assert!(parse_pps(&bytes).is_err());
}

#[test]
fn parse_pps_rejects_scaling_list_data_present() {
    let bytes = pps_bytes(false, false, false, true, false, 0);
    assert!(parse_pps(&bytes).is_err());
}

#[test]
fn parse_pps_echoes_lists_modification_present_flag() {
    let bytes = pps_bytes(false, false, false, false, true, 0);
    let pps = parse_pps(&bytes).expect("valid hand-built PPS");
    assert!(pps.lists_modification_present_flag);
}
