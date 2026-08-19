//! DXVA-shaped HEVC picture-parameter / slice-control / scaling-matrix structs and the
//! logic that packs parsed SPS/PPS/slice/POC/ref-list state into them.
//!
//! **Ground truth, cited (ADR-0004 § DXVA struct definitions)**: same absent-from-
//! `windows`-crate situation `h264_pic_params.rs` documents for H.264 —
//! `DXVA_PicParams_HEVC`/`DXVA_Slice_HEVC_Short`/`DXVA_Qmatrix_HEVC`/`DXVA_PicEntry_HEVC`
//! are absent from the pinned `windows` crate's generated bindings entirely (grepped, zero
//! matches). Hand-defined here, `repr(C)`, ground-truthed against the Wine project's
//! `dxva.h` mirror (fetched during ADR-0004's own design pass) — **not** independently
//! cross-checked against a third source (`libavcodec/dxva2_hevc.c`) this pass; ADR-0004's
//! own Open Question #2 flags this as the first implementation-time verification task
//! before any real hardware attempt.
//!
//! **`DXVA_PicParams_HEVC` carries no profile/tier/level field at all** — unlike H.264's
//! struct (which has none either, but for a different reason: HEVC's own accelerator
//! derives everything profile/tier/level-related from the raw VPS/SPS NAL bytes it
//! receives directly, same reasoning `hevc_vps_sps_pps.rs`'s module doc gives for not
//! parsing `profile_tier_level()`'s *values*, only skipping its bits).

use super::hevc_refs::HevcRefLists;
use super::hevc_slice::{SliceHeader, SliceType};
use super::hevc_vps_sps_pps::{Pps, Sps};

/// `DXVA_PicEntry_HEVC`: a 7-bit DPB slot index + 1-bit "long-term reference" flag,
/// packed into one byte — same shape/convention as H.264's `DxvaPicEntryH264`
/// (`h264_pic_params.rs`). `0xFF` marks an unused list entry; this module has no
/// long-term references (ADR-0004 § Scope decision), so the flag is always `0` for real
/// entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub(super) struct DxvaPicEntryHevc(pub(super) u8);

impl DxvaPicEntryHevc {
    pub(super) const UNUSED: Self = Self(0xFF);

    pub(super) const fn pack(index7_bits: u8) -> Self {
        Self(index7_bits & 0x7F)
    }
}

/// `DXVA_PicParams_HEVC` (Wine `dxva.h` mirror), `repr(C)` field-for-field. The
/// `union { struct { bitfields }; TYPE named; }` groups become plain integers (Rust has
/// no native C bitfields) — see the `pack_*` functions below for each group's bit layout.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct DxvaPicParamsHevc {
    pub(super) pic_width_in_min_cbs_y: u16,
    pub(super) pic_height_in_min_cbs_y: u16,
    pub(super) w_format_and_sequence_info_flags: u16,
    pub(super) curr_pic: DxvaPicEntryHevc,
    pub(super) sps_max_dec_pic_buffering_minus1: u8,
    pub(super) log2_min_luma_coding_block_size_minus3: u8,
    pub(super) log2_diff_max_min_luma_coding_block_size: u8,
    pub(super) log2_min_transform_block_size_minus2: u8,
    pub(super) log2_diff_max_min_transform_block_size: u8,
    pub(super) max_transform_hierarchy_depth_inter: u8,
    pub(super) max_transform_hierarchy_depth_intra: u8,
    pub(super) num_short_term_ref_pic_sets: u8,
    pub(super) num_long_term_ref_pics_sps: u8,
    pub(super) num_ref_idx_l0_default_active_minus1: u8,
    pub(super) num_ref_idx_l1_default_active_minus1: u8,
    pub(super) init_qp_minus26: i8,
    pub(super) uc_num_delta_pocs_of_ref_rps_idx: u8,
    pub(super) w_num_bits_for_short_term_rps_in_slice: u16,
    pub(super) reserved_bits2: u16,
    pub(super) dw_coding_param_tool_flags: u32,
    pub(super) dw_coding_setting_picture_property_flags: u32,
    pub(super) pps_cb_qp_offset: i8,
    pub(super) pps_cr_qp_offset: i8,
    pub(super) num_tile_columns_minus1: u8,
    pub(super) num_tile_rows_minus1: u8,
    pub(super) column_width_minus1: [u16; 19],
    pub(super) row_height_minus1: [u16; 21],
    pub(super) diff_cu_qp_delta_depth: u8,
    pub(super) pps_beta_offset_div2: i8,
    pub(super) pps_tc_offset_div2: i8,
    pub(super) log2_parallel_merge_level_minus2: u8,
    pub(super) curr_pic_order_cnt_val: i32,
    pub(super) ref_pic_list: [DxvaPicEntryHevc; 15],
    pub(super) reserved_bits5: u8,
    pub(super) pic_order_cnt_val_list: [i32; 15],
    pub(super) ref_pic_set_st_curr_before: [u8; 8],
    pub(super) ref_pic_set_st_curr_after: [u8; 8],
    pub(super) ref_pic_set_lt_curr: [u8; 8],
    pub(super) reserved_bits6: u16,
    pub(super) reserved_bits7: u16,
    pub(super) status_report_feedback_number: u32,
}

/// `DXVA_Slice_HEVC_Short` — a **much smaller field list than H.264's
/// `DXVA_Slice_H264_Long`**: no per-slice reference-list/weighted-prediction/QP detail at
/// all, since the D3D12 accelerator re-parses the entire slice-segment header itself from
/// the raw NAL bytes (see `hevc_slice.rs`'s module doc).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct DxvaSliceHevcShort {
    pub(super) bs_nal_unit_data_location: u32,
    pub(super) slice_bytes_in_buffer: u32,
    pub(super) w_bad_slice_chopping: u16,
}

/// `DXVA_Qmatrix_HEVC`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(
    clippy::struct_field_names,
    reason = "ucScalingLists0-3 are the real DXVA_Qmatrix_HEVC field names (Wine dxva.h \
    mirror) — renaming to drop the shared prefix would obscure the ground-truth mapping"
)]
pub(super) struct DxvaQmatrixHevc {
    pub(super) uc_scaling_lists0: [[u8; 16]; 6],
    pub(super) uc_scaling_lists1: [[u8; 64]; 6],
    pub(super) uc_scaling_lists2: [[u8; 64]; 6],
    pub(super) uc_scaling_lists3: [[u8; 64]; 2],
    pub(super) uc_scaling_list_dc_coef_size_id2: [u8; 6],
    pub(super) uc_scaling_list_dc_coef_size_id3: [u8; 2],
}

/// Flat (unscaled, all-16) `DXVA_Qmatrix_HEVC` — this module's scope rejects
/// `scaling_list_enabled_flag == 1` outright (ADR-0004 § Scope decision, § Alternatives
/// Considered: a stricter cut than H.264's own "parse but silently downgrade" fidelity
/// gap), so a flat matrix is always correct here, not a fidelity gap.
pub(super) const fn flat_qmatrix() -> DxvaQmatrixHevc {
    DxvaQmatrixHevc {
        uc_scaling_lists0: [[16u8; 16]; 6],
        uc_scaling_lists1: [[16u8; 64]; 6],
        uc_scaling_lists2: [[16u8; 64]; 6],
        uc_scaling_lists3: [[16u8; 64]; 2],
        uc_scaling_list_dc_coef_size_id2: [16u8; 6],
        uc_scaling_list_dc_coef_size_id3: [16u8; 2],
    }
}

/// Pack `wFormatAndSequenceInfoFlags` (16 bits total; bit 15 `ReservedBits1` is always
/// `0`, no term below sets it).
fn pack_format_and_sequence_info_flags(
    chroma_format_idc: u16,
    bit_depth_luma_minus8: u16,
    bit_depth_chroma_minus8: u16,
    log2_max_pic_order_cnt_lsb_minus4: u16,
    no_pic_reordering_flag: bool,
    no_bi_pred_flag: bool,
) -> u16 {
    (chroma_format_idc & 0b11)
        | ((bit_depth_luma_minus8 & 0b111) << 3)
        | ((bit_depth_chroma_minus8 & 0b111) << 6)
        | ((log2_max_pic_order_cnt_lsb_minus4 & 0b1111) << 9)
        | (u16::from(no_pic_reordering_flag) << 13)
        | (u16::from(no_bi_pred_flag) << 14)
}

/// Pack `dwCodingParamToolFlags` (32 bits total; bits 27-31 reserved, always `0`). This
/// module's scope forces `scaling_list_enabled_flag`/`pcm_enabled_flag`/
/// `long_term_ref_pics_present_flag` to `0` unconditionally (all rejected upstream if a
/// real stream signals them — `hevc_vps_sps_pps::parse_sps`), so those bits (plus every
/// PCM sub-field) are not real function parameters here, unlike the flags that vary per
/// stream.
#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "mirrors the DXVA bitfield layout 1:1 — each bool is one independent \
    bitfield, not a state machine, same reasoning h264_pic_params.rs's pack_bit_fields gives"
)]
fn pack_coding_param_tool_flags(
    amp_enabled_flag: bool,
    sample_adaptive_offset_enabled_flag: bool,
    sps_temporal_mvp_enabled_flag: bool,
    strong_intra_smoothing_enabled_flag: bool,
    dependent_slice_segments_enabled_flag: bool,
    output_flag_present_flag: bool,
    num_extra_slice_header_bits: u32,
    sign_data_hiding_enabled_flag: bool,
    cabac_init_present_flag: bool,
) -> u32 {
    (u32::from(amp_enabled_flag) << 1)
        | (u32::from(sample_adaptive_offset_enabled_flag) << 2)
        | (u32::from(sps_temporal_mvp_enabled_flag) << 18)
        | (u32::from(strong_intra_smoothing_enabled_flag) << 19)
        | (u32::from(dependent_slice_segments_enabled_flag) << 20)
        | (u32::from(output_flag_present_flag) << 21)
        | ((num_extra_slice_header_bits & 0b111) << 22)
        | (u32::from(sign_data_hiding_enabled_flag) << 25)
        | (u32::from(cabac_init_present_flag) << 26)
}

/// Pack `dwCodingSettingPicturePropertyFlags` (32 bits total; bits 19-31 reserved, always
/// `0`). `tiles_enabled_flag`/`entropy_coding_sync_enabled_flag`/
/// `deblocking_filter_override_enabled_flag`/`pps_deblocking_filter_disabled_flag` are
/// forced `0` unconditionally (all rejected upstream if signaled, or — for the two
/// deblocking bits — only meaningful when `deblocking_filter_control_present_flag == 1`,
/// which is itself rejected — `hevc_vps_sps_pps::parse_pps`), same reasoning as
/// [`pack_coding_param_tool_flags`].
#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "mirrors the DXVA bitfield layout 1:1, same reasoning as pack_coding_param_tool_flags"
)]
fn pack_coding_setting_picture_property_flags(
    constrained_intra_pred_flag: bool,
    transform_skip_enabled_flag: bool,
    cu_qp_delta_enabled_flag: bool,
    pps_slice_chroma_qp_offsets_present_flag: bool,
    weighted_pred_flag: bool,
    weighted_bipred_flag: bool,
    transquant_bypass_enabled_flag: bool,
    pps_loop_filter_across_slices_enabled_flag: bool,
    lists_modification_present_flag: bool,
    slice_segment_header_extension_present_flag: bool,
    irap_pic_flag: bool,
    idr_pic_flag: bool,
    intra_pic_flag: bool,
) -> u32 {
    u32::from(constrained_intra_pred_flag)
        | (u32::from(transform_skip_enabled_flag) << 1)
        | (u32::from(cu_qp_delta_enabled_flag) << 2)
        | (u32::from(pps_slice_chroma_qp_offsets_present_flag) << 3)
        | (u32::from(weighted_pred_flag) << 4)
        | (u32::from(weighted_bipred_flag) << 5)
        | (u32::from(transquant_bypass_enabled_flag) << 6)
        | (u32::from(pps_loop_filter_across_slices_enabled_flag) << 11)
        | (u32::from(lists_modification_present_flag) << 14)
        | (u32::from(slice_segment_header_extension_present_flag) << 15)
        | (u32::from(irap_pic_flag) << 16)
        | (u32::from(idr_pic_flag) << 17)
        | (u32::from(intra_pic_flag) << 18)
}

/// Build `DXVA_PicParams_HEVC` for the current picture.
///
/// `ref_lists` is this picture's constructed `RefPicList`/`RefPicSetStCurrBefore`/`After`
/// (`hevc_refs::build_ref_lists`, already reflecting the DPB **after** this picture's own
/// RPS-application eviction pass — see `hevc_refs::slots_to_evict`).
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one linear DXVA struct fill, mirrors h264_pic_params.rs::build_pic_params's identical shape"
)]
pub(super) fn build_pic_params(
    sps: &Sps,
    pps: &Pps,
    sh: &SliceHeader,
    curr_poc: i32,
    curr_pic_slot: u32,
    is_idr: bool,
    ref_lists: &HevcRefLists,
    status_report_feedback_number: u32,
) -> DxvaPicParamsHevc {
    let mut ref_pic_list = [DxvaPicEntryHevc::UNUSED; 15];
    let mut pic_order_cnt_val_list = [0i32; 15];
    for (i, (&slot, &poc)) in ref_lists
        .ref_pic_list
        .iter()
        .zip(ref_lists.poc_list.iter())
        .take(15)
        .enumerate()
    {
        ref_pic_list[i] = DxvaPicEntryHevc::pack(u8::try_from(slot).unwrap_or(0));
        pic_order_cnt_val_list[i] = poc;
    }
    let mut ref_pic_set_st_curr_before = [0xFFu8; 8];
    for (i, &idx) in ref_lists.st_curr_before.iter().take(8).enumerate() {
        ref_pic_set_st_curr_before[i] = idx;
    }
    let mut ref_pic_set_st_curr_after = [0xFFu8; 8];
    for (i, &idx) in ref_lists.st_curr_after.iter().take(8).enumerate() {
        ref_pic_set_st_curr_after[i] = idx;
    }

    let is_intra = matches!(sh.slice_type, SliceType::I);
    let w_format_and_sequence_info_flags = pack_format_and_sequence_info_flags(
        1, // chroma_format_idc (4:2:0 only, enforced by hevc_vps_sps_pps::parse_sps)
        0, // bit_depth_luma_minus8 (8-bit only, enforced by hevc_vps_sps_pps::parse_sps)
        0, // bit_depth_chroma_minus8 (ditto)
        u16::try_from(sps.log2_max_pic_order_cnt_lsb.saturating_sub(4)).unwrap_or(0),
        // NoPicReorderingFlag / NoBiPredFlag: always true — this module's scope has no
        // B-slices and single-forward-reference P only ever references the immediately
        // preceding decoded picture, so output order == decode order always (mirrors
        // d3d12_video_decode.rs's own H.264 module doc: "decodes and outputs pictures in
        // decode order, not display order").
        true,
        true,
    );
    let dw_coding_param_tool_flags = pack_coding_param_tool_flags(
        sps.amp_enabled_flag,
        sps.sample_adaptive_offset_enabled_flag,
        sps.sps_temporal_mvp_enabled_flag,
        sps.strong_intra_smoothing_enabled_flag,
        pps.dependent_slice_segments_enabled_flag,
        pps.output_flag_present_flag,
        pps.num_extra_slice_header_bits,
        pps.sign_data_hiding_enabled_flag,
        pps.cabac_init_present_flag,
    );
    let dw_coding_setting_picture_property_flags = pack_coding_setting_picture_property_flags(
        pps.constrained_intra_pred_flag,
        pps.transform_skip_enabled_flag,
        pps.cu_qp_delta_enabled_flag,
        pps.pps_slice_chroma_qp_offsets_present_flag,
        pps.weighted_pred_flag,
        pps.weighted_bipred_flag,
        pps.transquant_bypass_enabled_flag,
        pps.pps_loop_filter_across_slices_enabled_flag,
        pps.lists_modification_present_flag,
        pps.slice_segment_header_extension_present_flag,
        is_idr, // IrapPicFlag (CRA is rejected upstream, so IRAP == IDR in this scope)
        is_idr, // IdrPicFlag
        is_intra,
    );

    DxvaPicParamsHevc {
        pic_width_in_min_cbs_y: u16::try_from(
            sps.pic_width_in_luma_samples >> sps.log2_min_cb_size,
        )
        .unwrap_or(0),
        pic_height_in_min_cbs_y: u16::try_from(
            sps.pic_height_in_luma_samples >> sps.log2_min_cb_size,
        )
        .unwrap_or(0),
        w_format_and_sequence_info_flags,
        curr_pic: DxvaPicEntryHevc::pack(u8::try_from(curr_pic_slot).unwrap_or(0)),
        sps_max_dec_pic_buffering_minus1: u8::try_from(sps.max_dec_pic_buffering.saturating_sub(1))
            .unwrap_or(0),
        log2_min_luma_coding_block_size_minus3: u8::try_from(
            sps.log2_min_cb_size.saturating_sub(3),
        )
        .unwrap_or(0),
        log2_diff_max_min_luma_coding_block_size: u8::try_from(sps.log2_diff_max_min_cb_size)
            .unwrap_or(0),
        log2_min_transform_block_size_minus2: u8::try_from(sps.log2_min_tb_size.saturating_sub(2))
            .unwrap_or(0),
        log2_diff_max_min_transform_block_size: u8::try_from(sps.log2_diff_max_min_tb_size)
            .unwrap_or(0),
        max_transform_hierarchy_depth_inter: u8::try_from(sps.max_transform_hierarchy_depth_inter)
            .unwrap_or(0),
        max_transform_hierarchy_depth_intra: u8::try_from(sps.max_transform_hierarchy_depth_intra)
            .unwrap_or(0),
        // num_short_term_ref_pic_sets / num_long_term_ref_pics_sps: always 0 — SPS-level
        // RPS lists and long-term references are both rejected upstream (this module's
        // scope only ever has an inline, per-slice-signaled short-term RPS).
        num_short_term_ref_pic_sets: 0,
        num_long_term_ref_pics_sps: 0,
        num_ref_idx_l0_default_active_minus1: u8::try_from(
            pps.num_ref_idx_l0_default_active_minus1,
        )
        .unwrap_or(0),
        num_ref_idx_l1_default_active_minus1: u8::try_from(
            pps.num_ref_idx_l1_default_active_minus1,
        )
        .unwrap_or(0),
        init_qp_minus26: i8::try_from(pps.init_qp_minus26).unwrap_or(0),
        // ucNumDeltaPocsOfRefRpsIdx: only meaningful for inter-RPS-predicted entries
        // (`inter_ref_pic_set_prediction_flag == 1`), unreachable in this module's scope
        // (only `short_term_ref_pic_set(0)` is ever parsed, see hevc_slice.rs's module doc).
        uc_num_delta_pocs_of_ref_rps_idx: 0,
        w_num_bits_for_short_term_rps_in_slice: u16::try_from(sh.short_term_rps_bits)
            .unwrap_or(u16::MAX),
        reserved_bits2: 0,
        dw_coding_param_tool_flags,
        dw_coding_setting_picture_property_flags,
        pps_cb_qp_offset: i8::try_from(pps.pps_cb_qp_offset).unwrap_or(0),
        pps_cr_qp_offset: i8::try_from(pps.pps_cr_qp_offset).unwrap_or(0),
        // num_tile_columns/rows_minus1 + column_width/row_height: always 0 — single-tile
        // only, enforced by hevc_vps_sps_pps::parse_pps rejecting tiles_enabled_flag == 1.
        num_tile_columns_minus1: 0,
        num_tile_rows_minus1: 0,
        column_width_minus1: [0u16; 19],
        row_height_minus1: [0u16; 21],
        diff_cu_qp_delta_depth: u8::try_from(pps.diff_cu_qp_delta_depth).unwrap_or(0),
        // pps_beta/tc_offset_div2: default 0 — only meaningful when
        // deblocking_filter_control_present_flag == 1, which is rejected upstream.
        pps_beta_offset_div2: 0,
        pps_tc_offset_div2: 0,
        log2_parallel_merge_level_minus2: u8::try_from(pps.log2_parallel_merge_level_minus2)
            .unwrap_or(0),
        curr_pic_order_cnt_val: curr_poc,
        ref_pic_list,
        reserved_bits5: 0,
        pic_order_cnt_val_list,
        ref_pic_set_st_curr_before,
        ref_pic_set_st_curr_after,
        // No long-term references in this module's scope (ADR-0004 § Scope decision).
        ref_pic_set_lt_curr: [0xFFu8; 8],
        reserved_bits6: 0,
        reserved_bits7: 0,
        status_report_feedback_number,
    }
}

/// Build `DXVA_Slice_HEVC_Short` for the current (sole) slice of this picture.
///
/// `bs_nal_unit_data_location`/`slice_bytes_in_buffer` describe the slice NAL's real
/// position in the compressed-bitstream input buffer (`hevc_ops.rs` owns that layout) —
/// unlike H.264's slice-long struct, there is no `BitOffsetToSliceData`-equivalent field
/// here at all (see this file's module doc): the accelerator locates and parses
/// `slice_segment_header()`/`slice_segment_data()` itself from the raw bytes.
pub(super) const fn build_slice_short(
    bs_nal_unit_data_location: u32,
    slice_bytes_in_buffer: u32,
) -> DxvaSliceHevcShort {
    DxvaSliceHevcShort {
        bs_nal_unit_data_location,
        slice_bytes_in_buffer,
        w_bad_slice_chopping: 0,
    }
}

#[cfg(test)]
#[path = "hevc_pic_params_tests.rs"]
mod tests;
