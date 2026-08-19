//! Pure unit tests for [`super::build_pic_params`]/[`super::build_slice_short`]/
//! [`super::flat_qmatrix`] against hand-built `Sps`/`Pps`/`SliceHeader` fixtures, plus
//! `#[repr(C)]` size sanity checks (ADR-0004 § Test plan: since no `windows`-crate
//! reference struct exists to compare against via `std::mem::size_of`, unlike H.264's own
//! `h264_pic_params.rs`, which has no equivalent test file at all — these checks are
//! self-consistency only, not ground-truthed against a second, independent source).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{
    DxvaPicEntryHevc, DxvaPicParamsHevc, DxvaQmatrixHevc, build_pic_params, build_slice_short,
    flat_qmatrix,
};
use crate::windows::d3d12_video_decode::hevc_refs::HevcRefLists;
use crate::windows::d3d12_video_decode::hevc_slice::{SliceHeader, SliceType};
use crate::windows::d3d12_video_decode::hevc_vps_sps_pps::{Pps, Sps};

fn test_sps() -> Sps {
    Sps {
        pic_width_in_luma_samples: 384, // 6 * 64, exact MinCb multiple for log2_min_cb_size == 6
        pic_height_in_luma_samples: 320,
        log2_max_pic_order_cnt_lsb: 8,
        max_dec_pic_buffering: 4,
        log2_min_cb_size: 6,
        log2_diff_max_min_cb_size: 2,
        log2_min_tb_size: 2,
        log2_diff_max_min_tb_size: 3,
        max_transform_hierarchy_depth_inter: 2,
        max_transform_hierarchy_depth_intra: 1,
        amp_enabled_flag: true,
        sample_adaptive_offset_enabled_flag: true,
        sps_temporal_mvp_enabled_flag: false,
        strong_intra_smoothing_enabled_flag: false,
    }
}

fn test_pps() -> Pps {
    Pps {
        dependent_slice_segments_enabled_flag: false,
        output_flag_present_flag: false,
        num_extra_slice_header_bits: 0,
        sign_data_hiding_enabled_flag: true,
        cabac_init_present_flag: false,
        num_ref_idx_l0_default_active_minus1: 0,
        num_ref_idx_l1_default_active_minus1: 0,
        init_qp_minus26: -2,
        constrained_intra_pred_flag: false,
        transform_skip_enabled_flag: true,
        cu_qp_delta_enabled_flag: false,
        diff_cu_qp_delta_depth: 0,
        pps_cb_qp_offset: 1,
        pps_cr_qp_offset: -1,
        pps_slice_chroma_qp_offsets_present_flag: false,
        weighted_pred_flag: false,
        weighted_bipred_flag: false,
        transquant_bypass_enabled_flag: false,
        pps_loop_filter_across_slices_enabled_flag: true,
        lists_modification_present_flag: false,
        log2_parallel_merge_level_minus2: 0,
        slice_segment_header_extension_present_flag: false,
    }
}

fn test_slice_header(slice_type: SliceType) -> SliceHeader {
    SliceHeader {
        slice_type,
        pic_order_cnt_lsb: Some(3),
        short_term_rps: None,
        num_ref_idx_l0_active_minus1: 0,
        short_term_rps_bits: 7,
    }
}

#[test]
fn dxva_struct_sizes_are_internally_consistent() {
    // `DXVA_PicEntry_HEVC` is a one-byte union — mirrors `DxvaPicEntryH264`.
    assert_eq!(std::mem::size_of::<DxvaPicEntryHevc>(), 1);
    // 6*16 + 6*64 + 6*64 + 2*64 + 6 + 2 == 96 + 384 + 384 + 128 + 6 + 2 == 1000 bytes.
    assert_eq!(std::mem::size_of::<DxvaQmatrixHevc>(), 1000);
    // Sanity floor: the struct must be large enough to hold its two 15-entry arrays
    // (`RefPicList`/`PicOrderCntValList`) plus the three 8-entry `RefPicSet*` arrays —
    // 15*1 + 15*4 + 8*3 == 15 + 60 + 24 == 99 bytes, just from those five fields alone.
    assert!(std::mem::size_of::<DxvaPicParamsHevc>() >= 99);
}

#[test]
fn flat_qmatrix_is_all_sixteen() {
    let q = flat_qmatrix();
    assert!(
        q.uc_scaling_lists0
            .iter()
            .all(|list| list.iter().all(|&v| v == 16))
    );
    assert!(
        q.uc_scaling_lists3
            .iter()
            .all(|list| list.iter().all(|&v| v == 16))
    );
    assert!(q.uc_scaling_list_dc_coef_size_id2.iter().all(|&v| v == 16));
}

#[test]
fn build_slice_short_echoes_position_and_size() {
    let s = build_slice_short(0, 1234);
    assert_eq!(s.bs_nal_unit_data_location, 0);
    assert_eq!(s.slice_bytes_in_buffer, 1234);
    assert_eq!(s.w_bad_slice_chopping, 0);
}

#[test]
fn build_pic_params_packs_dimensions_and_current_picture() {
    let sps = test_sps();
    let pps = test_pps();
    let sh = test_slice_header(SliceType::I);
    let ref_lists = HevcRefLists::default();

    let pp = build_pic_params(&sps, &pps, &sh, 3, 7, true, &ref_lists, 42);

    // PicWidthInMinCbsY == pic_width_in_luma_samples >> log2_min_cb_size == 384 >> 6 == 6.
    assert_eq!(pp.pic_width_in_min_cbs_y, 6);
    assert_eq!(pp.pic_height_in_min_cbs_y, 5); // 320 >> 6 == 5
    assert_eq!(pp.curr_pic.0, 7); // curr_pic_slot, AssociatedFlag bit clear
    assert_eq!(pp.curr_pic_order_cnt_val, 3);
    assert_eq!(pp.status_report_feedback_number, 42);
    assert_eq!(pp.sps_max_dec_pic_buffering_minus1, 3); // max_dec_pic_buffering - 1
    assert_eq!(pp.log2_min_luma_coding_block_size_minus3, 3); // 6 - 3
    assert_eq!(pp.log2_diff_max_min_luma_coding_block_size, 2);
    assert_eq!(pp.pps_cb_qp_offset, 1);
    assert_eq!(pp.pps_cr_qp_offset, -1);
    assert_eq!(pp.init_qp_minus26, -2);
    // No long-term references in this module's scope — always all-unused.
    assert!(pp.ref_pic_set_lt_curr.iter().all(|&v| v == 0xFF));
}

#[test]
fn build_pic_params_format_and_sequence_info_flags_round_trip() {
    let sps = test_sps();
    let pps = test_pps();
    let sh = test_slice_header(SliceType::I);
    let ref_lists = HevcRefLists::default();
    let pp = build_pic_params(&sps, &pps, &sh, 0, 0, true, &ref_lists, 0);

    let flags = pp.w_format_and_sequence_info_flags;
    assert_eq!(flags & 0b11, 1); // chroma_format_idc == 1
    assert_eq!((flags >> 9) & 0b1111, 4); // log2_max_pic_order_cnt_lsb_minus4 == 8 - 4
    assert_eq!((flags >> 13) & 1, 1); // NoPicReorderingFlag always set
    assert_eq!((flags >> 14) & 1, 1); // NoBiPredFlag always set
}

#[test]
fn build_pic_params_coding_flags_echo_sps_pps() {
    let sps = test_sps();
    let pps = test_pps();
    let sh = test_slice_header(SliceType::I);
    let ref_lists = HevcRefLists::default();
    let pp = build_pic_params(&sps, &pps, &sh, 0, 0, false, &ref_lists, 0);

    let tool_flags = pp.dw_coding_param_tool_flags;
    assert_eq!((tool_flags >> 1) & 1, 1); // amp_enabled_flag == true
    assert_eq!((tool_flags >> 2) & 1, 1); // sample_adaptive_offset_enabled_flag == true
    assert_eq!((tool_flags >> 25) & 1, 1); // sign_data_hiding_enabled_flag == true

    let prop_flags = pp.dw_coding_setting_picture_property_flags;
    assert_eq!(prop_flags & 1, 0); // constrained_intra_pred_flag == false
    assert_eq!((prop_flags >> 1) & 1, 1); // transform_skip_enabled_flag == true
    assert_eq!((prop_flags >> 11) & 1, 1); // pps_loop_filter_across_slices_enabled_flag == true
    assert_eq!((prop_flags >> 17) & 1, 0); // IdrPicFlag == false (is_idr param was false)
    assert_eq!((prop_flags >> 18) & 1, 1); // IntraPicFlag == true (SliceType::I)
}

#[test]
fn build_pic_params_ref_lists_populate_ref_pic_list_and_st_curr() {
    let sps = test_sps();
    let pps = test_pps();
    let sh = test_slice_header(SliceType::P);
    let mut ref_lists = HevcRefLists::default();
    ref_lists.ref_pic_list.push(5);
    ref_lists.poc_list.push(-2);
    ref_lists.st_curr_before.push(0);

    let pp = build_pic_params(&sps, &pps, &sh, 3, 1, false, &ref_lists, 0);

    assert_eq!(pp.ref_pic_list[0].0, 5);
    assert_eq!(pp.pic_order_cnt_val_list[0], -2);
    assert_eq!(pp.ref_pic_set_st_curr_before[0], 0);
    assert_eq!(pp.ref_pic_set_st_curr_after[0], 0xFF);
    // Every unused RefPicList slot beyond what was populated stays UNUSED (0xFF).
    assert_eq!(pp.ref_pic_list[1].0, 0xFF);
}
