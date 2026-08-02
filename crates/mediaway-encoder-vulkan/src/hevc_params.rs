//! Minimal `StdVideoH265*` construction for one all-intra Main-profile frame
//! — mirrors [`super::h264_params`], but HEVC needs a third parameter set
//! (VPS) and every syntax element the H.265 spec adds beyond H.264 (CTU/TU
//! size range, `profile_tier_level`, DPB-management struct, slice-segment
//! header). Deliberately narrow: no VUI, no scaling lists, no SAO, no PCM,
//! no tiles, no B/P-frame fields, one temporal sub-layer, no long-term refs.
//!
//! Coding-tree/transform-unit size range (CTU `8x8..32x32`, TU `4x4..32x32`,
//! transform-hierarchy depth `3`) matches
//! `mediaway-encoder-windows`'s D3D12 HEVC backend's real-hardware-validated
//! choice (`hevc.rs::default_codec_config_hevc`) — the same legal range,
//! chosen here for consistency across this workspace's two independent HEVC
//! encode backends, not because this crate re-validated it against a Vulkan
//! driver's own codec-configuration query (no such query is exercised here;
//! `pProfileTierLevel`/CTU-TU range validity is confirmed by this crate's own
//! hardware-gated test instead, the same way H.264's fixed profile was).
//!
//! These are plain-old-data C structs (`vulkanalia::vk::native`), not `vulkanalia::vk::*Khr`
//! structs — see [`super::h264_params`]'s module doc for why constructing/
//! reading them needs no `unsafe`.

#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

use vulkanalia::vk::video as native;

/// A coded picture size already aligned to this backend's fixed minimum
/// coding-block size (`MinCbSizeY == 8`, see the module doc) — mirrors
/// [`super::h264_params::McAlignedExtent`]'s macroblock-alignment role, but
/// for HEVC's driver-reported (not hardcoded) `picture_access_granularity`
/// (validated by [`crate::session::Capabilities::validate_requested_extent`]
/// before this type is ever constructed, so `from_pixels` only re-checks the
/// weaker 8-pixel CTU-grid requirement this backend's fixed CU/TU range needs).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CtuAlignedExtent {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl CtuAlignedExtent {
    pub(crate) const fn from_pixels(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 || width % 8 != 0 || height % 8 != 0 {
            return None;
        }
        Some(Self { width, height })
    }
}

/// HEVC Main profile `general_profile_idc`.
const PROFILE_IDC_MAIN: native::StdVideoH265ProfileIdc = native::STD_VIDEO_H265_PROFILE_IDC_MAIN;
/// Level 1.0 — this backend never queries a driver-suggested level (Vulkan's
/// `VkVideoSessionCreateInfoKHR` has no level field the way D3D12's
/// `CreateVideoEncoderHeap` does; mirrors [`super::h264_params`]'s own fixed
/// Level 1.0 choice).
const LEVEL_IDC_1_0: native::StdVideoH265LevelIdc = native::STD_VIDEO_H265_LEVEL_IDC_1_0;

// CU/TU log2 sizes matching this file's module-doc CTU `8x8..32x32` / TU
// `4x4..32x32` range. `log2(8) == 3`, `log2(32) == 5`, `log2(4) == 2`.
const CB_MIN_LOG2_MINUS3: u8 = 0; // log2(8) - 3
const CB_DIFF_LOG2: u8 = 2; // log2(32) - log2(8)
const TB_MIN_LOG2_MINUS2: u8 = 0; // log2(4) - 2
const TB_DIFF_LOG2: u8 = 3; // log2(32) - log2(4)
const TRANSFORM_HIERARCHY_DEPTH: u8 = 3; // == TB_DIFF_LOG2, the legal maximum for this range

pub(crate) fn profile_tier_level_main() -> native::StdVideoH265ProfileTierLevel {
    let mut flags = native::StdVideoH265ProfileTierLevelFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
        __bindgen_padding_0: [0; 3],
    };
    flags.set_general_progressive_source_flag(1);
    flags.set_general_frame_only_constraint_flag(1);
    native::StdVideoH265ProfileTierLevel {
        flags,
        general_profile_idc: PROFILE_IDC_MAIN,
        general_level_idc: LEVEL_IDC_1_0,
    }
}

/// All-zero: no reference frames used this stage (every picture is
/// independent), matching `MaxDecPicBufferingMinus1[0] ==
/// MaxNumReorderPics[0] == MaxLatencyIncreasePlus1[0] == 0`.
pub(crate) const fn dec_pic_buf_mgr_no_refs() -> native::StdVideoH265DecPicBufMgr {
    native::StdVideoH265DecPicBufMgr {
        max_latency_increase_plus1: [0; 7],
        max_dec_pic_buffering_minus1: [0; 7],
        max_num_reorder_pics: [0; 7],
    }
}

/// `vps_video_parameter_set_id == 0`; single temporal sub-layer; no HRD/
/// timing info signaled.
pub(crate) fn build_vps(
    profile_tier_level: &native::StdVideoH265ProfileTierLevel,
    dec_pic_buf_mgr: &native::StdVideoH265DecPicBufMgr,
) -> native::StdVideoH265VideoParameterSet {
    let mut flags = native::StdVideoH265VpsFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
        __bindgen_padding_0: [0; 3],
    };
    flags.set_vps_temporal_id_nesting_flag(1);
    native::StdVideoH265VideoParameterSet {
        flags,
        vps_video_parameter_set_id: 0,
        vps_max_sub_layers_minus1: 0,
        reserved1: 0,
        reserved2: 0,
        vps_num_units_in_tick: 0,
        vps_time_scale: 0,
        vps_num_ticks_poc_diff_one_minus1: 0,
        reserved3: 0,
        pDecPicBufMgr: dec_pic_buf_mgr,
        pHrdParameters: std::ptr::null(),
        pProfileTierLevel: profile_tier_level,
    }
}

/// `sps_video_parameter_set_id == 0`, `sps_seq_parameter_set_id == 0`,
/// 4:2:0 8-bit (matches NV12), `log2_max_pic_order_cnt_lsb_minus4 == 0` (every
/// picture is independently an IDR — no meaningful POC LSB range needed).
pub(crate) const fn build_sps(
    extent: CtuAlignedExtent,
    profile_tier_level: &native::StdVideoH265ProfileTierLevel,
    dec_pic_buf_mgr: &native::StdVideoH265DecPicBufMgr,
) -> native::StdVideoH265SequenceParameterSet {
    native::StdVideoH265SequenceParameterSet {
        flags: native::StdVideoH265SpsFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        },
        chroma_format_idc: native::STD_VIDEO_H265_CHROMA_FORMAT_IDC_420,
        pic_width_in_luma_samples: extent.width,
        pic_height_in_luma_samples: extent.height,
        sps_video_parameter_set_id: 0,
        sps_max_sub_layers_minus1: 0,
        sps_seq_parameter_set_id: 0,
        bit_depth_luma_minus8: 0,
        bit_depth_chroma_minus8: 0,
        log2_max_pic_order_cnt_lsb_minus4: 0,
        log2_min_luma_coding_block_size_minus3: CB_MIN_LOG2_MINUS3,
        log2_diff_max_min_luma_coding_block_size: CB_DIFF_LOG2,
        log2_min_luma_transform_block_size_minus2: TB_MIN_LOG2_MINUS2,
        log2_diff_max_min_luma_transform_block_size: TB_DIFF_LOG2,
        max_transform_hierarchy_depth_inter: TRANSFORM_HIERARCHY_DEPTH,
        max_transform_hierarchy_depth_intra: TRANSFORM_HIERARCHY_DEPTH,
        num_short_term_ref_pic_sets: 0,
        num_long_term_ref_pics_sps: 0,
        pcm_sample_bit_depth_luma_minus1: 0,
        pcm_sample_bit_depth_chroma_minus1: 0,
        log2_min_pcm_luma_coding_block_size_minus3: 0,
        log2_diff_max_min_pcm_luma_coding_block_size: 0,
        reserved1: 0,
        reserved2: 0,
        palette_max_size: 0,
        delta_palette_max_predictor_size: 0,
        motion_vector_resolution_control_idc: 0,
        sps_num_palette_predictor_initializers_minus1: 0,
        conf_win_left_offset: 0,
        conf_win_right_offset: 0,
        conf_win_top_offset: 0,
        conf_win_bottom_offset: 0,
        pProfileTierLevel: profile_tier_level,
        pDecPicBufMgr: dec_pic_buf_mgr,
        pScalingLists: std::ptr::null(),
        pShortTermRefPicSet: std::ptr::null(),
        pLongTermRefPicsSps: std::ptr::null(),
        pSequenceParameterSetVui: std::ptr::null(),
        pPredictorPaletteEntries: std::ptr::null(),
    }
}

/// `pps_pic_parameter_set_id == 0`, matching `build_sps`'s
/// `sps_seq_parameter_set_id == 0`. `init_qp_minus26 == 0` → initial QP 26
/// (unused directly — the slice sets `constant_qp` explicitly, mirrors
/// [`super::h264_params::build_pps`]). No tiles, deblocking left at spec
/// defaults (`pps_loop_filter_across_slices_enabled_flag == 1`, matching
/// `mediaway-encoder-windows`'s D3D12 HEVC PPS choice).
pub(crate) fn build_pps() -> native::StdVideoH265PictureParameterSet {
    let mut flags = native::StdVideoH265PpsFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_pps_loop_filter_across_slices_enabled_flag(1);
    native::StdVideoH265PictureParameterSet {
        flags,
        pps_pic_parameter_set_id: 0,
        pps_seq_parameter_set_id: 0,
        sps_video_parameter_set_id: 0,
        num_extra_slice_header_bits: 0,
        num_ref_idx_l0_default_active_minus1: 0,
        num_ref_idx_l1_default_active_minus1: 0,
        init_qp_minus26: 0,
        diff_cu_qp_delta_depth: 0,
        pps_cb_qp_offset: 0,
        pps_cr_qp_offset: 0,
        pps_beta_offset_div2: 0,
        pps_tc_offset_div2: 0,
        log2_parallel_merge_level_minus2: 0,
        log2_max_transform_skip_block_size_minus2: 0,
        diff_cu_chroma_qp_offset_depth: 0,
        chroma_qp_offset_list_len_minus1: 0,
        cb_qp_offset_list: [0; 6],
        cr_qp_offset_list: [0; 6],
        log2_sao_offset_scale_luma: 0,
        log2_sao_offset_scale_chroma: 0,
        pps_act_y_qp_offset_plus5: 0,
        pps_act_cb_qp_offset_plus5: 0,
        pps_act_cr_qp_offset_plus3: 0,
        pps_num_palette_predictor_initializers: 0,
        luma_bit_depth_entry_minus8: 0,
        chroma_bit_depth_entry_minus8: 0,
        num_tile_columns_minus1: 0,
        num_tile_rows_minus1: 0,
        reserved1: 0,
        reserved2: 0,
        column_width_minus1: [0; 19],
        row_height_minus1: [0; 21],
        reserved3: 0,
        pScalingLists: std::ptr::null(),
        pPredictorPaletteEntries: std::ptr::null(),
    }
}

/// `StdVideoEncodeH265ReferenceListsInfo` with zero active references — same
/// "valid but empty, not null" reasoning as
/// [`super::h264_params::build_empty_reference_lists`].
pub(crate) const fn build_empty_reference_lists() -> native::StdVideoEncodeH265ReferenceListsInfo {
    native::StdVideoEncodeH265ReferenceListsInfo {
        flags: native::StdVideoEncodeH265ReferenceListsInfoFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        },
        num_ref_idx_l0_active_minus1: 0,
        num_ref_idx_l1_active_minus1: 0,
        RefPicList0: [0xFF; 15],
        RefPicList1: [0xFF; 15],
        list_entry_l0: [0xFF; 15],
        list_entry_l1: [0xFF; 15],
    }
}

/// `StdVideoEncodeH265PictureInfo` for frame 0 of an IDR-only stream —
/// `pRefLists` is left null here, same reasoning as
/// [`super::h264_params::build_idr_picture_info`]: the caller wires it to a
/// co-located `StdVideoEncodeH265ReferenceListsInfo` it keeps alive on its
/// own stack frame.
pub(crate) fn build_idr_picture_info() -> native::StdVideoEncodeH265PictureInfo {
    let mut flags = native::StdVideoEncodeH265PictureInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_is_reference(1);
    flags.set_IrapPicFlag(1);
    flags.set_pic_output_flag(1);
    native::StdVideoEncodeH265PictureInfo {
        flags,
        pic_type: native::STD_VIDEO_H265_PICTURE_TYPE_IDR,
        sps_video_parameter_set_id: 0,
        pps_seq_parameter_set_id: 0,
        pps_pic_parameter_set_id: 0,
        short_term_ref_pic_set_idx: 0,
        PicOrderCntVal: 0,
        TemporalId: 0,
        reserved1: [0; 7],
        pRefLists: std::ptr::null(),
        pShortTermRefPicSet: std::ptr::null(),
        pLongTermRefPics: std::ptr::null(),
    }
}

/// I-slice-segment header covering the whole picture
/// (`first_slice_segment_in_pic_flag == 1`, `slice_segment_address == 0`),
/// `slice_loop_filter_across_slices_enabled_flag == 1` matching
/// [`build_pps`]'s PPS-level choice. `MaxNumMergeCand` is meaningless for an
/// I-slice (no inter prediction) but must still hold the spec-legal `1..=5`
/// range — set to the maximum.
pub(crate) fn build_idr_slice_segment_header() -> native::StdVideoEncodeH265SliceSegmentHeader {
    let mut flags = native::StdVideoEncodeH265SliceSegmentHeaderFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_first_slice_segment_in_pic_flag(1);
    flags.set_slice_loop_filter_across_slices_enabled_flag(1);
    native::StdVideoEncodeH265SliceSegmentHeader {
        flags,
        slice_type: native::STD_VIDEO_H265_SLICE_TYPE_I,
        slice_segment_address: 0,
        collocated_ref_idx: 0,
        MaxNumMergeCand: 5,
        slice_cb_qp_offset: 0,
        slice_cr_qp_offset: 0,
        slice_beta_offset_div2: 0,
        slice_tc_offset_div2: 0,
        slice_act_y_qp_offset: 0,
        slice_act_cb_qp_offset: 0,
        slice_act_cr_qp_offset: 0,
        slice_qp_delta: 0,
        reserved1: 0,
        pWeightTable: std::ptr::null(),
    }
}
