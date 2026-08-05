//! Minimal `StdVideoH265*` construction for one all-intra Main-profile frame
//! — mirrors [`super::h264_params`], but HEVC needs a third parameter set
//! (VPS) and every syntax element the H.265 spec adds beyond H.264 (CTU/TU
//! size range, `profile_tier_level`, DPB-management struct, slice-segment
//! header). Deliberately narrow: no VUI, no scaling lists, no SAO, no PCM,
//! no tiles, one temporal sub-layer, no long-term refs.
//!
//! ADR-0002 adds P-frame picture-info/slice-segment-header/reference-list
//! builders alongside the original IDR-only ones, mirroring
//! [`super::h264_params`]'s own P-frame additions — a P-frame's single L0
//! reference is signaled via a picture-embedded
//! `StdVideoH265ShortTermRefPicSet` (`short_term_ref_pic_set_sps_flag ==
//! 0`), not an SPS-declared entry (`num_short_term_ref_pic_sets` stays `0`
//! in every SPS this crate builds, GOP or not) — this crate's single
//! forward-reference design needs at most one short-term RPS per frame, so
//! there is nothing an SPS-level RPS list would buy over signaling it
//! per-picture.
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
/// (validated by [`crate::vulkan::session::Capabilities::validate_requested_extent`]
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

/// GOP mode's `StdVideoH265DecPicBufMgr` (ADR-0002) — sub-layer 0's
/// `MaxDecPicBufferingMinus1 == 1` (DPB holds up to 2 pictures: the current
/// one plus this crate's single forward reference), `MaxNumReorderPics ==
/// 0` (no B-frames, no reordering — permanent non-goal, see
/// [`super::hevc_gop`]'s module doc), `MaxLatencyIncreasePlus1 == 0` (no
/// latency constraint signaled). Every other sub-layer stays `0`
/// (`sps_max_sub_layers_minus1 == 0`, only index `0` is ever read).
pub(crate) const fn dec_pic_buf_mgr_single_ref() -> native::StdVideoH265DecPicBufMgr {
    let mut max_dec_pic_buffering_minus1 = [0u8; 7];
    max_dec_pic_buffering_minus1[0] = 1;
    native::StdVideoH265DecPicBufMgr {
        max_latency_increase_plus1: [0; 7],
        max_dec_pic_buffering_minus1,
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

/// GOP-dependent SPS fields (ADR-0002) — mirrors
/// [`h264_params::SpsGopParams`](super::h264_params::SpsGopParams).
/// [`Self::IDR_ONLY`] reproduces Stage 1's SPS bytes exactly (every picture
/// independently an IDR, no meaningful POC LSB range needed); GOP mode uses
/// [`hevc_gop::LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4`](crate::vulkan::hevc_gop::LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4).
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpsGopParams {
    pub(crate) log2_max_pic_order_cnt_lsb_minus4: u8,
}

impl SpsGopParams {
    /// Stage 1's exact value.
    pub(crate) const IDR_ONLY: Self = Self {
        log2_max_pic_order_cnt_lsb_minus4: 0,
    };
}

/// `sps_video_parameter_set_id == 0`, `sps_seq_parameter_set_id == 0`,
/// 4:2:0 8-bit (matches NV12). `gop` selects Stage 1's IDR-only
/// `log2_max_pic_order_cnt_lsb_minus4` or ADR-0002's GOP-enabled one — see
/// [`SpsGopParams`].
pub(crate) const fn build_sps(
    extent: CtuAlignedExtent,
    profile_tier_level: &native::StdVideoH265ProfileTierLevel,
    dec_pic_buf_mgr: &native::StdVideoH265DecPicBufMgr,
    gop: SpsGopParams,
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
        log2_max_pic_order_cnt_lsb_minus4: gop.log2_max_pic_order_cnt_lsb_minus4,
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

/// `StdVideoEncodeH265ReferenceListsInfo` with exactly one active L0 entry
/// pointing at `ref_slot` (a DPB slot index, matching
/// `STD_VIDEO_H265_NO_REFERENCE_PICTURE` (`0xFF`) sentinel semantics
/// [`build_empty_reference_lists`] already relies on for its unused
/// entries) — mirrors
/// [`h264_params::build_single_reference_list`](super::h264_params::build_single_reference_list):
/// this crate's single forward-reference design, no L1, no reference-list
/// modification (`list_entry_l0`/`list_entry_l1` stay the `0xFF` sentinel
/// from [`build_empty_reference_lists`], unused since
/// `ref_pic_list_modification_flag_l0/l1` are never set).
pub(crate) const fn build_single_reference_list(
    ref_slot: u8,
) -> native::StdVideoEncodeH265ReferenceListsInfo {
    let mut list = build_empty_reference_lists();
    list.num_ref_idx_l0_active_minus1 = 0;
    list.RefPicList0[0] = ref_slot;
    list
}

/// All-zero: no long-term marking — mirrors
/// [`h264_params::reference_info_flags`](super::h264_params::reference_info_flags).
const fn reference_info_flags() -> native::StdVideoEncodeH265ReferenceInfoFlags {
    native::StdVideoEncodeH265ReferenceInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    }
}

/// `StdVideoEncodeH265ReferenceInfo` describing one already-encoded (or
/// about-to-be-encoded) picture, for whichever DPB slot it lives in — HEVC
/// sibling of
/// [`h264_params::build_reference_info`](super::h264_params::build_reference_info),
/// simpler since HEVC's reference-info struct has no `FrameNum` equivalent
/// (`PicOrderCntVal` is the only ordering value this crate signals — see
/// [`super::hevc_gop::DpbSlot`]).
pub(crate) const fn build_reference_info(
    poc: i32,
    is_idr: bool,
) -> native::StdVideoEncodeH265ReferenceInfo {
    native::StdVideoEncodeH265ReferenceInfo {
        flags: reference_info_flags(),
        pic_type: if is_idr {
            native::STD_VIDEO_H265_PICTURE_TYPE_IDR
        } else {
            native::STD_VIDEO_H265_PICTURE_TYPE_P
        },
        PicOrderCntVal: poc,
        TemporalId: 0,
    }
}

/// All-zero `StdVideoH265ShortTermRefPicSet`: `num_negative_pics ==
/// num_positive_pics == 0`, no entries — this crate's IDR picture-info
/// leaves `pShortTermRefPicSet` null (H.265 §7.3.6.1: an IDR slice-segment
/// header carries no RPS syntax at all), so this value is only ever built
/// for [`FrameStdStructs`]'s uniform "always populated, sometimes unused"
/// shape (mirrors
/// [`h264_params::FrameStdStructs::setup_reference_info`](super::h264_params::FrameStdStructs)'s
/// same convention) — never actually pointed to by an IDR picture's
/// `pShortTermRefPicSet`.
const fn build_empty_short_term_ref_pic_set() -> native::StdVideoH265ShortTermRefPicSet {
    native::StdVideoH265ShortTermRefPicSet {
        flags: native::StdVideoH265ShortTermRefPicSetFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
            __bindgen_padding_0: [0; 3],
        },
        delta_idx_minus1: 0,
        use_delta_flag: 0,
        abs_delta_rps_minus1: 0,
        used_by_curr_pic_flag: 0,
        used_by_curr_pic_s0_flag: 0,
        used_by_curr_pic_s1_flag: 0,
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
        num_negative_pics: 0,
        num_positive_pics: 0,
        delta_poc_s0_minus1: [0; 16],
        delta_poc_s1_minus1: [0; 16],
    }
}

/// A P-frame's picture-embedded short-term RPS (H.265 §7.3.6.1/§7.4.8,
/// `inter_ref_pic_set_prediction_flag == 0`, direct signaling): exactly one
/// negative-direction entry, the immediately preceding picture (this
/// crate's sole L0 reference, one `PicOrderCntVal` behind the current
/// picture — see [`super::hevc_gop::GopState`]). `DeltaPocS0[0] ==
/// -(delta_poc_s0_minus1[0] + 1) == -1`, so `delta_poc_s0_minus1[0] == 0`;
/// `used_by_curr_pic_s0_flag` bit `0` set marks that one entry as an active
/// reference for this slice.
const fn build_single_ref_short_term_ref_pic_set() -> native::StdVideoH265ShortTermRefPicSet {
    let mut rps = build_empty_short_term_ref_pic_set();
    rps.num_negative_pics = 1;
    rps.used_by_curr_pic_s0_flag = 1;
    rps
}

/// `IdrPicFlag`-equivalent unset, `is_reference` + `pic_output_flag` set —
/// every frame in this crate's single-forward-reference GOP design becomes
/// a candidate reference for the frame immediately after it (see
/// [`super::hevc_gop::GopState`]); mirrors
/// [`h264_params::p_picture_info_flags`](super::h264_params::p_picture_info_flags).
fn p_picture_info_flags() -> native::StdVideoEncodeH265PictureInfoFlags {
    let mut flags = native::StdVideoEncodeH265PictureInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_is_reference(1);
    flags.set_pic_output_flag(1);
    flags
}

/// P-slice-segment header covering the whole picture — otherwise identical
/// to [`build_idr_slice_segment_header`] (full-picture single slice
/// segment, default deblocking, no weighted prediction, no temporal MVP);
/// only `slice_type` differs. `num_ref_idx_active_override_flag` stays
/// unset — this crate's single active L0 reference matches
/// [`build_pps`]'s `num_ref_idx_l0_default_active_minus1 == 0` default, so
/// no per-slice override is needed.
fn build_p_slice_segment_header() -> native::StdVideoEncodeH265SliceSegmentHeader {
    let mut flags = native::StdVideoEncodeH265SliceSegmentHeaderFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_first_slice_segment_in_pic_flag(1);
    flags.set_slice_loop_filter_across_slices_enabled_flag(1);
    native::StdVideoEncodeH265SliceSegmentHeader {
        flags,
        slice_type: native::STD_VIDEO_H265_SLICE_TYPE_P,
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

/// One frame's full set of `StdVideoH265*` per-frame structs, resolved from
/// [`crate::vulkan::hevc_gop::GopState::decide`]'s output — HEVC sibling of
/// [`h264_params::FrameStdStructs`](super::h264_params::FrameStdStructs)/
/// [`h264_params::build_frame_structs`](super::h264_params::build_frame_structs).
/// `gop_size == 1` callers always pass `is_idr: true, reference_slot: None`
/// (every `GopState::decide` call under `gop_size == 1` returns exactly
/// that), reproducing Stage 1's [`build_idr_picture_info`]/
/// [`build_empty_reference_lists`]/[`build_idr_slice_segment_header`]
/// byte-for-byte.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameStdStructs {
    pub(crate) picture_info: native::StdVideoEncodeH265PictureInfo,
    pub(crate) reference_lists: native::StdVideoEncodeH265ReferenceListsInfo,
    pub(crate) slice_segment_header: native::StdVideoEncodeH265SliceSegmentHeader,
    /// `None` for an IDR frame (an IDR slice-segment header carries no RPS
    /// syntax — see [`build_empty_short_term_ref_pic_set`]'s doc) — `Some`
    /// for a P-frame, the caller wires it to `picture_info.pShortTermRefPicSet`
    /// (kept out-of-line here since `picture_info` cannot hold a
    /// self-referential pointer across a function return, same reasoning as
    /// `pRefLists`).
    pub(crate) short_term_ref_pic_set: Option<native::StdVideoH265ShortTermRefPicSet>,
    /// This frame's own `StdVideoEncodeH265ReferenceInfo`, for the DPB slot
    /// it is about to occupy — always populated (even for `gop_size == 1`,
    /// where no later frame ever reads it back), mirrors
    /// [`h264_params::FrameStdStructs::setup_reference_info`](super::h264_params::FrameStdStructs).
    pub(crate) setup_reference_info: native::StdVideoEncodeH265ReferenceInfo,
}

pub(crate) fn build_frame_structs(
    poc: i32,
    is_idr: bool,
    reference_slot: Option<u8>,
) -> FrameStdStructs {
    // The IDR branch reuses [`build_idr_picture_info`] wholesale (unlike
    // `h264_params::build_frame_structs`, which reimplements the IDR flags
    // inline) — this crate has no HEVC equivalent of
    // `session_encode::encode_synthetic_intra_frame` (H.264's Stage 1
    // one-shot diagnostic) to keep `build_idr_picture_info` alive as a
    // second caller, so `build_frame_structs` is its only remaining call
    // site; `PicOrderCntVal` is hardcoded `0` there, matching `poc`'s own
    // value on every IDR call (`GopState::decide` resets `poc` to `0` at
    // every IDR — see `hevc_gop.rs`).
    let picture_info = if is_idr {
        build_idr_picture_info()
    } else {
        native::StdVideoEncodeH265PictureInfo {
            flags: p_picture_info_flags(),
            pic_type: native::STD_VIDEO_H265_PICTURE_TYPE_P,
            sps_video_parameter_set_id: 0,
            pps_seq_parameter_set_id: 0,
            pps_pic_parameter_set_id: 0,
            short_term_ref_pic_set_idx: 0,
            PicOrderCntVal: poc,
            TemporalId: 0,
            reserved1: [0; 7],
            pRefLists: std::ptr::null(),
            pShortTermRefPicSet: std::ptr::null(),
            pLongTermRefPics: std::ptr::null(),
        }
    };
    let reference_lists =
        reference_slot.map_or_else(build_empty_reference_lists, build_single_reference_list);
    let slice_segment_header = if is_idr {
        build_idr_slice_segment_header()
    } else {
        build_p_slice_segment_header()
    };
    let short_term_ref_pic_set = (!is_idr).then(build_single_ref_short_term_ref_pic_set);
    let setup_reference_info = build_reference_info(poc, is_idr);
    FrameStdStructs {
        picture_info,
        reference_lists,
        slice_segment_header,
        short_term_ref_pic_set,
        setup_reference_info,
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
