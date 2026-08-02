//! Minimal `StdVideoH264*` construction for **one** all-intra Baseline-profile
//! frame — the smallest self-consistent SPS/PPS/picture-info/slice-header set
//! `VkVideoEncodeH264SessionParametersCreateInfoKHR` and `vkCmdEncodeVideoKHR`
//! need for Stage 1's single synthetic IDR frame. See
//! `adr/0001-vulkan-video-encode-ash-probe.md`'s 2026-07-29 addendum for the
//! design this mirrors.
//!
//! Deliberately narrow: no VUI, no scaling lists, no B/P-frame fields, POC
//! type 2 (derived from `frame_num`, needs no explicit POC signaling) — not a
//! general-purpose H.264 parameter-set builder.
//!
//! These are plain-old-data C structs (`vulkanalia::vk::native`), not `vulkanalia::vk::*Khr`
//! structs — they have no `s_type`/`p_next` and constructing/reading them is
//! not itself `unsafe` (only dereferencing the raw pointers embedded in a few
//! of them would be). No `unsafe` blocks appear in this file.

#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

use vulkanalia::vk::video as native;

/// A coded picture size already aligned to the H.264 macroblock grid (both
/// dimensions multiples of 16), expressed in macroblock units.
#[derive(Debug, Clone, Copy)]
pub(crate) struct McAlignedExtent {
    pub(crate) width_mbs: u32,
    pub(crate) height_mbs: u32,
}

impl McAlignedExtent {
    /// Fails if `width`/`height` are zero or not 16-aligned — every H.264
    /// coded extent (`VkVideoCapabilitiesKHR::pictureAccessGranularity` is
    /// 16x16 on every driver this crate has queried) must be macroblock
    /// aligned, so an unaligned extent is a caller bug worth rejecting early
    /// rather than silently truncating.
    pub(crate) const fn from_pixels(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 || width % 16 != 0 || height % 16 != 0 {
            return None;
        }
        Some(Self {
            width_mbs: width / 16,
            height_mbs: height / 16,
        })
    }
}

/// `direct_8x8_inference_flag` and `frame_mbs_only_flag` set (progressive,
/// no field coding); every other SPS flag stays `0` — this crate never
/// signals separate colour planes, scaling matrices, VUI, or B/P prediction.
fn sps_flags(
    direct_8x8_inference_flag: bool,
    frame_mbs_only_flag: bool,
) -> native::StdVideoH264SpsFlags {
    let mut flags = native::StdVideoH264SpsFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 2]),
        __bindgen_padding_0: 0,
    };
    flags.set_direct_8x8_inference_flag(u32::from(direct_8x8_inference_flag));
    flags.set_frame_mbs_only_flag(u32::from(frame_mbs_only_flag));
    flags
}

/// All-zero PPS flags: `entropy_coding_mode_flag = 0` (CAVLC — the only mode
/// Baseline profile permits), default deblocking, no weighted prediction, no
/// scaling-matrix override.
const fn pps_flags() -> native::StdVideoH264PpsFlags {
    native::StdVideoH264PpsFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
        __bindgen_padding_0: [0; 3],
    }
}

/// `IdrPicFlag` + `is_reference` set; no long-term marking, no adaptive ref
/// marking — the shape every IDR picture needs regardless of GOP structure.
fn idr_picture_info_flags() -> native::StdVideoEncodeH264PictureInfoFlags {
    let mut flags = native::StdVideoEncodeH264PictureInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_IdrPicFlag(1);
    flags.set_is_reference(1);
    flags
}

/// All-zero: no reference-list overrides, no spatial direct prediction (both
/// meaningless for an I-slice).
const fn slice_header_flags() -> native::StdVideoEncodeH264SliceHeaderFlags {
    native::StdVideoEncodeH264SliceHeaderFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    }
}

/// All-zero: no reference-list modification.
const fn reference_lists_info_flags() -> native::StdVideoEncodeH264ReferenceListsInfoFlags {
    native::StdVideoEncodeH264ReferenceListsInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    }
}

/// Baseline-profile SPS for one macroblock-aligned coded picture size.
/// `seq_parameter_set_id = 0`; POC type 2 (no explicit POC signaling needed);
/// `max_num_ref_frames = 0` (this crate's frame is never referenced).
pub(crate) fn build_sps(extent: McAlignedExtent) -> native::StdVideoH264SequenceParameterSet {
    native::StdVideoH264SequenceParameterSet {
        flags: sps_flags(true, true),
        profile_idc: native::STD_VIDEO_H264_PROFILE_IDC_BASELINE,
        level_idc: native::STD_VIDEO_H264_LEVEL_IDC_1_0,
        chroma_format_idc: native::STD_VIDEO_H264_CHROMA_FORMAT_IDC_420,
        seq_parameter_set_id: 0,
        bit_depth_luma_minus8: 0,
        bit_depth_chroma_minus8: 0,
        log2_max_frame_num_minus4: 0,
        pic_order_cnt_type: native::STD_VIDEO_H264_POC_TYPE_2,
        offset_for_non_ref_pic: 0,
        offset_for_top_to_bottom_field: 0,
        log2_max_pic_order_cnt_lsb_minus4: 0,
        num_ref_frames_in_pic_order_cnt_cycle: 0,
        max_num_ref_frames: 0,
        reserved1: 0,
        pic_width_in_mbs_minus1: extent.width_mbs - 1,
        pic_height_in_map_units_minus1: extent.height_mbs - 1,
        frame_crop_left_offset: 0,
        frame_crop_right_offset: 0,
        frame_crop_top_offset: 0,
        frame_crop_bottom_offset: 0,
        reserved2: 0,
        pOffsetForRefFrame: core::ptr::null(),
        pScalingLists: core::ptr::null(),
        pSequenceParameterSetVui: core::ptr::null(),
    }
}

/// Baseline-profile PPS, `pic_parameter_set_id = 0`, matching `build_sps`'s
/// `seq_parameter_set_id = 0`. `pic_init_qp_minus26 = 0` → initial QP 26
/// (mid-range within the driver's reported `minQp..=maxQp`, unused directly
/// since the slice sets `constant_qp` explicitly — see `session.rs`).
pub(crate) const fn build_pps() -> native::StdVideoH264PictureParameterSet {
    native::StdVideoH264PictureParameterSet {
        flags: pps_flags(),
        seq_parameter_set_id: 0,
        pic_parameter_set_id: 0,
        num_ref_idx_l0_default_active_minus1: 0,
        num_ref_idx_l1_default_active_minus1: 0,
        weighted_bipred_idc: native::StdVideoH264WeightedBipredIdc(0),
        pic_init_qp_minus26: 0,
        pic_init_qs_minus26: 0,
        chroma_qp_index_offset: 0,
        second_chroma_qp_index_offset: 0,
        pScalingLists: core::ptr::null(),
    }
}

/// `StdVideoEncodeH264ReferenceListsInfo` with zero active references — still
/// provided (not left null) for the IDR picture info's `pRefLists`, since
/// this crate could not verify against the (unavailable, see ADR) validation
/// layer whether a null `pRefLists` is accepted for IDR pictures by every
/// driver; a valid-but-empty struct is the safer choice either way.
pub(crate) const fn build_empty_reference_lists() -> native::StdVideoEncodeH264ReferenceListsInfo {
    native::StdVideoEncodeH264ReferenceListsInfo {
        flags: reference_lists_info_flags(),
        num_ref_idx_l0_active_minus1: 0,
        num_ref_idx_l1_active_minus1: 0,
        RefPicList0: [0xFF; 32],
        RefPicList1: [0xFF; 32],
        refList0ModOpCount: 0,
        refList1ModOpCount: 0,
        refPicMarkingOpCount: 0,
        reserved1: [0; 7],
        pRefList0ModOperations: core::ptr::null(),
        pRefList1ModOperations: core::ptr::null(),
        pRefPicMarkingOperations: core::ptr::null(),
    }
}

/// `StdVideoEncodeH264PictureInfo` for frame 0 of an IDR-only stream.
/// `pRefLists` is left null here — the caller (`session.rs`) wires it to a
/// co-located `StdVideoEncodeH264ReferenceListsInfo` it keeps alive on its
/// own stack frame, since a function return here cannot hand back a
/// self-referential pointer.
pub(crate) fn build_idr_picture_info() -> native::StdVideoEncodeH264PictureInfo {
    native::StdVideoEncodeH264PictureInfo {
        flags: idr_picture_info_flags(),
        seq_parameter_set_id: 0,
        pic_parameter_set_id: 0,
        idr_pic_id: 0,
        primary_pic_type: native::STD_VIDEO_H264_PICTURE_TYPE_IDR,
        frame_num: 0,
        PicOrderCnt: 0,
        temporal_id: 0,
        reserved1: [0; 3],
        pRefLists: core::ptr::null(),
    }
}

/// I-slice header covering the whole picture (`first_mb_in_slice = 0`),
/// default deblocking (`disable_deblocking_filter_idc = 0`, offsets 0),
/// CAVLC `cabac_init_idc` is unused (entropy coding mode is CAVLC per
/// [`pps_flags`]) but must still hold a defined value.
pub(crate) const fn build_idr_slice_header() -> native::StdVideoEncodeH264SliceHeader {
    native::StdVideoEncodeH264SliceHeader {
        flags: slice_header_flags(),
        first_mb_in_slice: 0,
        slice_type: native::STD_VIDEO_H264_SLICE_TYPE_I,
        slice_alpha_c0_offset_div2: 0,
        slice_beta_offset_div2: 0,
        slice_qp_delta: 0,
        reserved1: 0,
        cabac_init_idc: native::StdVideoH264CabacInitIdc(0),
        disable_deblocking_filter_idc: native::StdVideoH264DisableDeblockingFilterIdc(0),
        pWeightTable: core::ptr::null(),
    }
}
