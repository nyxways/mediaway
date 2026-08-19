//! Pure unit tests for [`super::build_pic_params`]/[`super::build_tile`] against hand-built
//! `SequenceHeader`/`FrameHeader` fixtures, plus `repr(C)` size sanity checks (ADR-0005 §
//! Test plan: no `windows`-crate reference struct exists to compare against via
//! `std::mem::size_of`, same situation `hevc_pic_params_tests.rs` documents for HEVC —
//! these checks are self-consistency only, not ground-truthed against a second,
//! independent source).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{DxvaPicEntryAv1, DxvaQuantizationAv1, DxvaTileAv1, build_pic_params, build_tile};
use crate::windows::d3d12_video_decode::av1_frame_header::{
    FrameHeader, LoopFilter, Quantization, TileInfo,
};
use crate::windows::d3d12_video_decode::av1_sequence_header::SequenceHeader;

fn test_seq() -> SequenceHeader {
    SequenceHeader {
        max_frame_width: 352,
        max_frame_height: 288,
        frame_width_bits: 9,
        frame_height_bits: 9,
        use_128x128_superblock: false,
        enable_filter_intra: true,
        enable_intra_edge_filter: true,
        enable_interintra_compound: false,
        enable_masked_compound: false,
        enable_dual_filter: false,
        enable_order_hint: false,
        enable_jnt_comp: false,
        enable_ref_frame_mvs: false,
        order_hint_bits: 0,
        separate_uv_delta_q: false,
    }
}

fn test_frame_header() -> FrameHeader {
    FrameHeader {
        width: 352,
        height: 288,
        disable_cdf_update: true,
        disable_frame_end_update_cdf: true,
        order_hint: 0,
        quantization: Quantization {
            base_q_idx: 60,
            delta_q_y_dc: -2,
            delta_q_u_dc: 0,
            delta_q_u_ac: 0,
            delta_q_v_dc: 0,
            delta_q_v_ac: 0,
        },
        delta_q_present: false,
        delta_q_res: 0,
        delta_lf_present: false,
        delta_lf_res: 0,
        delta_lf_multi: false,
        loop_filter: LoopFilter {
            level: [0, 0],
            level_u: 0,
            level_v: 0,
            sharpness: 0,
            delta_enabled: false,
            delta_update: false,
            ref_deltas: [1, 0, 0, 0, -1, 0, 0, -1],
            mode_deltas: [0, 0],
        },
        tx_mode: 1,
        reduced_tx_set: false,
        tile: TileInfo {
            tile_width_sb: 6,
            tile_height_sb: 5,
        },
    }
}

#[test]
fn dxva_struct_sizes_are_internally_consistent() {
    // `DXVA_PicEntry_AV1`: width(4)+height(4)+wmmat[6](24)+global_motion_flags(1)+index(1)
    // +Reserved16Bits(2) == 36 bytes, no interior padding (every field already lands on
    // its own natural alignment boundary).
    assert_eq!(std::mem::size_of::<DxvaPicEntryAv1>(), 36);
    // `DXVA_Tile_AV1`: DataOffset(4)+DataSize(4)+row(2)+column(2)+Reserved16Bits(2)+
    // anchor_frame(1)+Reserved8Bits(1) == 16 bytes.
    assert_eq!(std::mem::size_of::<DxvaTileAv1>(), 16);
    // `DXVA_PicParams_AV1.quantization`: ControlFlags(1)+base_qindex(1)+5 delta bytes(5)+
    // qm_y/u/v(3)+Reserved16Bits(2, 2-byte aligned) == 12 bytes.
    assert_eq!(std::mem::size_of::<DxvaQuantizationAv1>(), 12);
}

#[test]
fn frame_refs_and_ref_frame_map_are_always_unused() {
    let pp = build_pic_params(&test_seq(), &test_frame_header(), 3, 1);
    for entry in &pp.frame_refs {
        assert_eq!(
            entry.index, 0xFF,
            "no picture is ever referenced in this scope"
        );
    }
    for &index in &pp.ref_frame_map_texture_index {
        assert_eq!(index, 0xFF);
    }
}

#[test]
fn build_pic_params_maps_scalar_fields() {
    let pp = build_pic_params(&test_seq(), &test_frame_header(), 3, 7);
    assert_eq!(pp.width, 352);
    assert_eq!(pp.height, 288);
    assert_eq!(pp.max_width, 352);
    assert_eq!(pp.max_height, 288);
    assert_eq!(pp.curr_pic_texture_index, 3);
    assert_eq!(pp.superres_denom, 8);
    assert_eq!(pp.bitdepth, 8);
    assert_eq!(pp.seq_profile, 0);
    assert_eq!(pp.primary_ref_frame, 7); // PRIMARY_REF_NONE
    assert_eq!(pp.status_report_feedback_number, 7);
    assert_eq!(pp.tiles.cols, 1);
    assert_eq!(pp.tiles.rows, 1);
    assert_eq!(pp.tiles.widths[0], 6);
    assert_eq!(pp.tiles.heights[0], 5);
    assert_eq!(pp.quantization.base_qindex, 60);
    assert_eq!(pp.quantization.y_dc_delta_q, -2);
    // using_qmatrix is always false in this module's scope -> 0xFF sentinel.
    assert_eq!(pp.quantization.qm_y, 0xFF);
    assert_eq!(pp.quantization.qm_u, 0xFF);
    assert_eq!(pp.quantization.qm_v, 0xFF);
}

#[test]
fn build_pic_params_packs_coding_param_tool_flags() {
    let pp = build_pic_params(&test_seq(), &test_frame_header(), 0, 1);
    // bit 8 (integer_mv, force_integer_mv) is always set -- FrameIsIntra always forces it.
    assert_ne!(pp.coding_param_tool_flags & (1 << 8), 0);
    // bit 15 (filter_intra) mirrors seq.enable_filter_intra == true in this fixture.
    assert_ne!(pp.coding_param_tool_flags & (1 << 15), 0);
    // bit 9 (cdef) / bit 10 (restoration) / bit 21 (superres) are always unset in this
    // module's scope.
    assert_eq!(pp.coding_param_tool_flags & (1 << 9), 0);
    assert_eq!(pp.coding_param_tool_flags & (1 << 10), 0);
    assert_eq!(pp.coding_param_tool_flags & (1 << 21), 0);
    // bits 22-23 carry TxMode == 1 (TX_MODE_LARGEST, this fixture's frame header).
    assert_eq!((pp.coding_param_tool_flags >> 22) & 0b11, 1);
    // bit 26 (reference_frame_update) is always set (show_existing_frame == 0 always).
    assert_ne!(pp.coding_param_tool_flags & (1 << 26), 0);
}

#[test]
fn build_pic_params_packs_format_and_picture_info_flags() {
    let pp = build_pic_params(&test_seq(), &test_frame_header(), 0, 1);
    let flags = pp.format_and_picture_info_flags;
    assert_eq!(flags & 0b11, 0); // frame_type == KEY_FRAME(0)
    assert_ne!(flags & (1 << 2), 0); // show_frame == 1
    assert_eq!(flags & (1 << 3), 0); // showable_frame == 0 (KEY_FRAME)
    assert_ne!(flags & (1 << 4), 0); // subsampling_x == 1
    assert_ne!(flags & (1 << 5), 0); // subsampling_y == 1
    assert_eq!(flags & (1 << 6), 0); // mono_chrome == 0
}

#[test]
fn build_pic_params_maps_loop_filter_ref_deltas() {
    let pp = build_pic_params(&test_seq(), &test_frame_header(), 0, 1);
    assert_eq!(pp.loop_filter.ref_deltas, [1, 0, 0, 0, -1, 0, 0, -1]);
    assert_eq!(pp.loop_filter.mode_deltas, [0, 0]);
    assert_eq!(pp.loop_filter.filter_level, [0, 0]);
}

#[test]
fn build_tile_marks_no_tile_list_membership() {
    let tile = build_tile(0, 4096);
    assert_eq!(tile.data_offset, 0);
    assert_eq!(tile.data_size, 4096);
    assert_eq!(tile.anchor_frame, 0xFF);
}
