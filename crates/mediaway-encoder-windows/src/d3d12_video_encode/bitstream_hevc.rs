//! Minimal HEVC Annex-B VPS/SPS/PPS writer for the D3D12 native video-encode backend.
//!
//! Mirrors [`super::bitstream`]'s H.264 SPS/PPS writer, but HEVC needs a third parameter
//! set (VPS) and a **2-byte** NAL header (`forbidden_zero_bit`(1) + `nal_unit_type`(6) +
//! `nuh_layer_id`(6) + `nuh_temporal_id_plus1`(3), Rec. ITU-T H.265 §7.3.1.2) instead of
//! H.264's 1-byte header — different enough that generalizing [`super::bitstream`]'s NAL
//! wrapper wasn't worth it; the RBSP bit writer and emulation-prevention logic are shared
//! (see [`super::bitstream::RbspWriter`], [`super::bitstream::push_rbsp_with_emulation_prevention`]).
//!
//! Scope matches this backend's all-intra/no-reference/single-layer configuration: Main
//! profile (`general_profile_idc == 1`), one temporal sub-layer
//! (`sps_max_sub_layers_minus1 == 0`), no scaling lists, no PCM, no tiles/WPP, no VUI, no
//! SPS/PPS range extensions. Field values ground-truthed against Rec. ITU-T H.265 §7.3.3
//! (`profile_tier_level`), §7.3.2.1 (`video_parameter_set_rbsp`), §7.3.2.2
//! (`seq_parameter_set_rbsp`), §7.3.2.3 (`pic_parameter_set_rbsp`).

#![forbid(unsafe_code)]

use super::bitstream::{RbspWriter, push_rbsp_with_emulation_prevention};

/// HEVC Main profile `general_profile_idc`.
const PROFILE_IDC_MAIN: u8 = 1;

const NAL_TYPE_VPS: u8 = 32;
const NAL_TYPE_SPS: u8 = 33;
const NAL_TYPE_PPS: u8 = 34;

// Coding-unit / transform-unit `log2` sizes matching
// [`super::hevc::default_codec_config_hevc`]'s fixed `8x8..32x32` CU / `4x4..32x32` TU
// range (that file's doc comment explains why this is a hardcoded, real-hardware-validated
// choice rather than a driver query). `log2(8) == 3`, `log2(32) == 5`, `log2(4) == 2`.
const CB_MIN_LOG2: u32 = 3;
const CB_DIFF_LOG2: u32 = 2; // log2(32) - log2(8)
const TB_MIN_LOG2: u32 = 2;
const TB_DIFF_LOG2: u32 = 3; // log2(32) - log2(4)
const TRANSFORM_HIERARCHY_DEPTH: u32 = 3; // == TB_DIFF_LOG2, the legal maximum for this range

/// `profile_tier_level(profilePresentFlag=1, maxNumSubLayersMinus1=0)` for HEVC Main
/// profile (Rec. ITU-T H.265 §7.3.3) — 12 bytes: `general_profile_space`(2) +
/// `general_tier_flag`(1) + `general_profile_idc`(5) +
/// `general_profile_compatibility_flag[32]`(32) + 4 single-bit source/constraint flags +
/// `general_reserved_zero_43bits`(43, Main-profile branch) + `general_inbld_flag`(1,
/// present since `profile_idc` is in `1..=5`) + `general_level_idc`(8). No sub-layer
/// profile/level fields since `maxNumSubLayersMinus1 == 0`.
fn write_profile_tier_level_main(w: &mut RbspWriter, general_tier_flag: u8, general_level_idc: u8) {
    w.write_bits(0, 2); // general_profile_space
    w.write_bit(general_tier_flag);
    w.write_bits(u32::from(PROFILE_IDC_MAIN), 5); // general_profile_idc
    let compat = 1u32 << (31 - u32::from(PROFILE_IDC_MAIN)); // general_profile_compatibility_flag[1] = 1
    w.write_bits(compat, 32);
    w.write_bit(1); // general_progressive_source_flag
    w.write_bit(0); // general_interlaced_source_flag
    w.write_bit(0); // general_non_packed_constraint_flag
    w.write_bit(1); // general_frame_only_constraint_flag
    w.write_zero_bits(43); // general_reserved_zero_43bits (Main profile: neither range-extension nor SCC branch)
    w.write_bit(0); // general_inbld_flag — no interlace/BLD capability claimed
    w.write_u8(general_level_idc);
}

fn write_vps(w: &mut RbspWriter, general_tier_flag: u8, general_level_idc: u8) {
    w.write_bits(0, 4); // vps_video_parameter_set_id
    w.write_bit(1); // vps_base_layer_internal_flag
    w.write_bit(1); // vps_base_layer_available_flag
    w.write_bits(0, 6); // vps_max_layers_minus1 — single layer
    w.write_bits(0, 3); // vps_max_sub_layers_minus1 — single temporal sub-layer
    w.write_bit(1); // vps_temporal_id_nesting_flag
    w.write_bits(0xffff, 16); // vps_reserved_0xffff_16bits
    write_profile_tier_level_main(w, general_tier_flag, general_level_idc);
    w.write_bit(0); // vps_sub_layer_ordering_info_present_flag
    w.write_ue(0); // vps_max_dec_pic_buffering_minus1[0] — no reference frames used this stage
    w.write_ue(0); // vps_max_num_reorder_pics[0]
    w.write_ue(0); // vps_max_latency_increase_plus1[0]
    w.write_bits(0, 6); // vps_max_layer_id
    w.write_ue(0); // vps_num_layer_sets_minus1
    w.write_bit(0); // vps_timing_info_present_flag
    w.write_bit(0); // vps_extension_flag
    w.rbsp_trailing_bits();
}

fn write_sps(
    w: &mut RbspWriter,
    width: u32,
    height: u32,
    general_tier_flag: u8,
    general_level_idc: u8,
) {
    w.write_bits(0, 4); // sps_video_parameter_set_id
    w.write_bits(0, 3); // sps_max_sub_layers_minus1
    w.write_bit(1); // sps_temporal_id_nesting_flag
    write_profile_tier_level_main(w, general_tier_flag, general_level_idc);
    w.write_ue(0); // sps_seq_parameter_set_id
    w.write_ue(1); // chroma_format_idc == 1 (4:2:0, matches NV12)
    w.write_ue(width); // pic_width_in_luma_samples
    w.write_ue(height); // pic_height_in_luma_samples
    w.write_bit(0); // conformance_window_flag — caller guarantees CTU-aligned width/height
    w.write_ue(0); // bit_depth_luma_minus8
    w.write_ue(0); // bit_depth_chroma_minus8
    w.write_ue(0); // log2_max_pic_order_cnt_lsb_minus4 — unused: every picture is IDR (no POC LSB signaled)
    w.write_bit(0); // sps_sub_layer_ordering_info_present_flag
    w.write_ue(0); // sps_max_dec_pic_buffering_minus1[0]
    w.write_ue(0); // sps_max_num_reorder_pics[0]
    w.write_ue(0); // sps_max_latency_increase_plus1[0]
    w.write_ue(CB_MIN_LOG2 - 3); // log2_min_luma_coding_block_size_minus3 (CU 8x8)
    w.write_ue(CB_DIFF_LOG2); // log2_diff_max_min_luma_coding_block_size (CU 8x8..32x32)
    w.write_ue(TB_MIN_LOG2 - 2); // log2_min_luma_transform_block_size_minus2 (TU 4x4)
    w.write_ue(TB_DIFF_LOG2); // log2_diff_max_min_luma_transform_block_size (TU 4x4..32x32)
    w.write_ue(TRANSFORM_HIERARCHY_DEPTH); // max_transform_hierarchy_depth_inter
    w.write_ue(TRANSFORM_HIERARCHY_DEPTH); // max_transform_hierarchy_depth_intra
    w.write_bit(0); // scaling_list_enabled_flag
    w.write_bit(0); // amp_enabled_flag
    w.write_bit(0); // sample_adaptive_offset_enabled_flag
    w.write_bit(0); // pcm_enabled_flag
    w.write_ue(0); // num_short_term_ref_pic_sets
    w.write_bit(0); // long_term_ref_pics_present_flag
    w.write_bit(0); // sps_temporal_mvp_enabled_flag
    w.write_bit(0); // strong_intra_smoothing_enabled_flag
    w.write_bit(0); // vui_parameters_present_flag
    w.write_bit(0); // sps_extension_present_flag
    w.rbsp_trailing_bits();
}

fn write_pps(w: &mut RbspWriter) {
    w.write_ue(0); // pps_pic_parameter_set_id
    w.write_ue(0); // pps_seq_parameter_set_id
    w.write_bit(0); // dependent_slice_segments_enabled_flag
    w.write_bit(0); // output_flag_present_flag
    w.write_bits(0, 3); // num_extra_slice_header_bits
    w.write_bit(0); // sign_data_hiding_enabled_flag
    w.write_bit(0); // cabac_init_present_flag
    w.write_ue(0); // num_ref_idx_l0_default_active_minus1
    w.write_ue(0); // num_ref_idx_l1_default_active_minus1
    w.write_se(0); // init_qp_minus26 — actual QP comes from D3D12 CQP rate control
    w.write_bit(0); // constrained_intra_pred_flag
    w.write_bit(0); // transform_skip_enabled_flag
    w.write_bit(0); // cu_qp_delta_enabled_flag
    w.write_se(0); // pps_cb_qp_offset
    w.write_se(0); // pps_cr_qp_offset
    w.write_bit(0); // pps_slice_chroma_qp_offsets_present_flag
    w.write_bit(0); // weighted_pred_flag
    w.write_bit(0); // weighted_bipred_flag
    w.write_bit(0); // transquant_bypass_enabled_flag
    w.write_bit(0); // tiles_enabled_flag
    w.write_bit(0); // entropy_coding_sync_enabled_flag
    w.write_bit(1); // pps_loop_filter_across_slices_enabled_flag
    w.write_bit(0); // deblocking_filter_control_present_flag — use spec defaults (deblocking on)
    w.write_bit(0); // pps_scaling_list_data_present_flag
    w.write_bit(0); // lists_modification_present_flag
    w.write_ue(0); // log2_parallel_merge_level_minus2
    w.write_bit(0); // slice_segment_header_extension_present_flag
    w.write_bit(0); // pps_extension_present_flag
    w.rbsp_trailing_bits();
}

/// Wrap RBSP bytes in a 2-byte HEVC NAL header (`nuh_layer_id == 0`,
/// `nuh_temporal_id_plus1 == 1`), apply emulation prevention, and prepend an Annex-B 4-byte
/// start code.
fn annex_b_nal_hevc(nal_unit_type: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len() + rbsp.len() / 2 + 6);
    // byte0 = forbidden_zero_bit(0) | nal_unit_type(6) | nuh_layer_id[5] (0);
    // byte1 = nuh_layer_id[4:0] (0) | nuh_temporal_id_plus1(3) (== 1, TemporalId 0).
    out.extend_from_slice(&[0, 0, 0, 1, nal_unit_type << 1, 0x01]);
    push_rbsp_with_emulation_prevention(&mut out, rbsp);
    out
}

/// Build the Annex-B VPS + SPS + PPS byte sequence for one HEVC encode session.
///
/// `width`/`height` are the actual pixel dimensions (caller validates alignment to
/// [`super::hevc::MIN_CB_SIZE_PIXELS`] before calling). `general_tier_flag`/
/// `general_level_idc` come from the driver's `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT`
/// `SuggestedLevel` (see [`super::hevc::level_hevc_to_general_level_idc`]) — a hardcoded
/// level is exactly the H.264-side mistake ADR-0007 already documents failing
/// `CreateVideoEncoderHeap`.
pub(super) fn build_hevc_headers(
    width: u32,
    height: u32,
    general_tier_flag: u8,
    general_level_idc: u8,
) -> Vec<u8> {
    let mut vps_w = RbspWriter::new();
    write_vps(&mut vps_w, general_tier_flag, general_level_idc);
    let vps_rbsp = vps_w.finish();

    let mut sps_w = RbspWriter::new();
    write_sps(
        &mut sps_w,
        width,
        height,
        general_tier_flag,
        general_level_idc,
    );
    let sps_rbsp = sps_w.finish();

    let mut pps_w = RbspWriter::new();
    write_pps(&mut pps_w);
    let pps_rbsp = pps_w.finish();

    let mut out = annex_b_nal_hevc(NAL_TYPE_VPS, &vps_rbsp);
    out.extend(annex_b_nal_hevc(NAL_TYPE_SPS, &sps_rbsp));
    out.extend(annex_b_nal_hevc(NAL_TYPE_PPS, &pps_rbsp));
    out
}
