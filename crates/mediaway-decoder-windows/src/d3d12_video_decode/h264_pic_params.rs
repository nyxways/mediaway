//! DXVA-shaped H.264 picture-parameter / slice-control / scaling-matrix structs and the
//! logic that packs parsed SPS/PPS/slice/POC/ref-list state into them.
//!
//! **ADR-0002 Open Question #1, resolved**: the pinned `windows` crate (0.62.2) exposes
//! the D3D12 video-decode *plumbing* (`ID3D12VideoDevice`, `ID3D12VideoDecoder`,
//! `ID3D12VideoDecoderHeap`, `ID3D12VideoDecodeCommandList1::DecodeFrame1`,
//! `D3D12_VIDEO_DECODE_REFERENCE_FRAMES`, `D3D12_VIDEO_DECODE_ARGUMENT_TYPE_*`) but
//! **not** the DXVA-specification per-codec picture-parameter structs themselves
//! (`DXVA_PicParams_H264`, `DXVA_Slice_H264_Long`, `DXVA_Qmatrix_H264` are absent from
//! the crate's generated bindings entirely — confirmed by grepping the vendored
//! `windows-0.62.2` source for every such symbol, none found). `D3D12_VIDEO_DECODE_
//! FRAME_ARGUMENT::pData` is only ever a `*mut c_void` + `Size` — the caller is expected
//! to supply a byte-identical DXVA struct from elsewhere. This module hand-defines them,
//! `repr(C)`, ground-truthed against the real Windows SDK `dxva.h` layout (fetched from
//! the Wine project's header mirror, which tracks Microsoft's public struct layout
//! byte-for-byte — reference-only, no code copied, same convention ADR-0007 used for
//! `FFmpeg` call-sequence grounding).

use super::h264_refs::RefListEntry;
use super::h264_slice::{SliceHeader, SliceType};
use super::h264_sps_pps::{Pps, Sps};

/// `DXVA_PicEntry_H264`: a 7-bit DPB slot index + 1-bit "long-term reference" flag,
/// packed into one byte. `0xFF` (all bits set) marks an unused list entry — this
/// module has no long-term references (ADR-0002 § Scope), so the flag is always `0`
/// for real entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub(super) struct DxvaPicEntryH264(pub(super) u8);

impl DxvaPicEntryH264 {
    pub(super) const UNUSED: Self = Self(0xFF);

    pub(super) const fn pack(index7_bits: u8, long_term: bool) -> Self {
        let base = index7_bits & 0x7F;
        Self(if long_term { base | 0x80 } else { base })
    }
}

/// `DXVA_PicParams_H264` (Windows SDK `dxva.h`), `repr(C)` field-for-field.
///
/// The `union { struct{ bitfields }; USHORT wBitFields; }` becomes a plain `u16` here
/// (Rust has no native C bitfields) — see [`pack_bit_fields`] for the bit layout.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct DxvaPicParamsH264 {
    pub(super) w_frame_width_in_mbs_minus1: u16,
    pub(super) w_frame_height_in_mbs_minus1: u16,
    pub(super) curr_pic: DxvaPicEntryH264,
    pub(super) num_ref_frames: u8,
    pub(super) w_bit_fields: u16,
    pub(super) bit_depth_luma_minus8: u8,
    pub(super) bit_depth_chroma_minus8: u8,
    pub(super) reserved16_bits: u16,
    pub(super) status_report_feedback_number: u32,
    pub(super) ref_frame_list: [DxvaPicEntryH264; 16],
    pub(super) curr_field_order_cnt: [i32; 2],
    pub(super) field_order_cnt_list: [[i32; 2]; 16],
    pub(super) pic_init_qs_minus26: i8,
    pub(super) chroma_qp_index_offset: i8,
    pub(super) second_chroma_qp_index_offset: i8,
    pub(super) continuation_flag: u8,
    pub(super) pic_init_qp_minus26: i8,
    pub(super) num_ref_idx_l0_active_minus1: u8,
    pub(super) num_ref_idx_l1_active_minus1: u8,
    pub(super) reserved8_bits_a: u8,
    pub(super) frame_num_list: [u16; 16],
    pub(super) used_for_reference_flags: u32,
    pub(super) non_existing_frame_flags: u16,
    pub(super) frame_num: u16,
    pub(super) log2_max_frame_num_minus4: u8,
    pub(super) pic_order_cnt_type: u8,
    pub(super) log2_max_pic_order_cnt_lsb_minus4: u8,
    pub(super) delta_pic_order_always_zero_flag: u8,
    pub(super) direct_8x8_inference_flag: u8,
    pub(super) entropy_coding_mode_flag: u8,
    pub(super) pic_order_present_flag: u8,
    pub(super) num_slice_groups_minus1: u8,
    pub(super) slice_group_map_type: u8,
    pub(super) deblocking_filter_control_present_flag: u8,
    pub(super) redundant_pic_cnt_present_flag: u8,
    pub(super) reserved8_bits_b: u8,
    pub(super) slice_group_change_rate_minus1: u16,
    pub(super) slice_group_map: [u8; 810],
}

/// `DXVA_Qmatrix_H264`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct DxvaQmatrixH264 {
    pub(super) scaling_lists_4x4: [[u8; 16]; 6],
    pub(super) scaling_lists_8x8: [[u8; 64]; 2],
}

/// `DXVA_Slice_H264_Long`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct DxvaSliceH264Long {
    pub(super) bs_nal_unit_data_location: u32,
    pub(super) slice_bytes_in_buffer: u32,
    pub(super) w_bad_slice_chopping: u16,
    pub(super) first_mb_in_slice: u16,
    pub(super) num_mbs_for_slice: u16,
    pub(super) bit_offset_to_slice_data: u16,
    pub(super) slice_type: u8,
    pub(super) luma_log2_weight_denom: u8,
    pub(super) chroma_log2_weight_denom: u8,
    pub(super) num_ref_idx_l0_active_minus1: u8,
    pub(super) num_ref_idx_l1_active_minus1: u8,
    pub(super) slice_alpha_c0_offset_div2: i8,
    pub(super) slice_beta_offset_div2: i8,
    pub(super) reserved8_bits: u8,
    pub(super) ref_pic_list: [[DxvaPicEntryH264; 32]; 2],
    pub(super) weights: [[[[i16; 2]; 3]; 32]; 2],
    pub(super) slice_qs_delta: i8,
    pub(super) slice_qp_delta: i8,
    pub(super) redundant_pic_cnt: u8,
    pub(super) direct_spatial_mv_pred_flag: u8,
    pub(super) cabac_init_idc: u8,
    pub(super) disable_deblocking_filter_idc: u8,
    pub(super) slice_id: u16,
}

/// Pack `DXVA_PicParams_H264::wBitFields` (§ see struct doc — no native Rust bitfields).
///
/// Bits 2 (`residual_colour_transform_flag`) and 3 (`sp_for_switch_flag`) are omitted
/// from the OR-chain below (an explicit `| (0 << n)` term is a clippy `identity_op` —
/// a no-op that changes nothing): both are always `0` here — 4:2:0-only with no
/// separate colour plane (bit 2), and SP slices are rejected outright by
/// `h264_slice::parse_slice_header` before reaching this function (bit 3).
#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "mirrors the DXVA bitfield layout 1:1 — each bool is one independent bitfield, not a state machine"
)]
fn pack_bit_fields(
    field_pic_flag: bool,
    mbaff_frame_flag: bool,
    chroma_format_idc: u16,
    ref_pic_flag: bool,
    constrained_intra_pred_flag: bool,
    weighted_pred_flag: bool,
    weighted_bipred_idc: u16,
    mbs_consecutive_flag: bool,
    frame_mbs_only_flag: bool,
    transform_8x8_mode_flag: bool,
    min_luma_bipred_size8x8_flag: bool,
    intra_pic_flag: bool,
) -> u16 {
    u16::from(field_pic_flag)
        | (u16::from(mbaff_frame_flag) << 1)
        | ((chroma_format_idc & 0b11) << 4)
        | (u16::from(ref_pic_flag) << 6)
        | (u16::from(constrained_intra_pred_flag) << 7)
        | (u16::from(weighted_pred_flag) << 8)
        | ((weighted_bipred_idc & 0b11) << 9)
        | (u16::from(mbs_consecutive_flag) << 11)
        | (u16::from(frame_mbs_only_flag) << 12)
        | (u16::from(transform_8x8_mode_flag) << 13)
        | (u16::from(min_luma_bipred_size8x8_flag) << 14)
        | (u16::from(intra_pic_flag) << 15)
}

/// Build `DXVA_PicParams_H264` for the current picture.
///
/// `refs` is the DPB's active-reference set *before* this picture is added (its own
/// slot is `curr_pic_slot`, not included in `refs`).
#[allow(
    clippy::too_many_arguments,
    reason = "one linear DXVA struct fill, mirrors FFmpeg's own dxva2_h264.c fill_picture_parameters shape"
)]
pub(super) fn build_pic_params(
    sps: &Sps,
    pps: &Pps,
    sh: &SliceHeader,
    curr_top_poc: i32,
    curr_bottom_poc: i32,
    curr_pic_slot: u32,
    nal_ref_idc: u8,
    frame_num: u32,
    refs: &[(u32, super::h264_refs::H264RefMeta)],
    status_report_feedback_number: u32,
) -> DxvaPicParamsH264 {
    let mut ref_frame_list = [DxvaPicEntryH264::UNUSED; 16];
    let mut field_order_cnt_list = [[0i32; 2]; 16];
    let mut frame_num_list = [0u16; 16];
    let mut used_for_reference_flags = 0u32;
    for (i, &(slot, meta)) in refs.iter().take(16).enumerate() {
        ref_frame_list[i] = DxvaPicEntryH264::pack(u8::try_from(slot).unwrap_or(0), false);
        field_order_cnt_list[i] = [meta.top_field_order_cnt, meta.bottom_field_order_cnt];
        frame_num_list[i] = u16::try_from(meta.frame_num).unwrap_or(0);
        used_for_reference_flags |= 0b11 << (i * 2);
    }

    let is_intra_only = matches!(sh.slice_type, SliceType::I);
    let w_bit_fields = pack_bit_fields(
        false, // field_pic_flag
        false, // MbaffFrameFlag
        1,     // chroma_format_idc (4:2:0 only, enforced by h264_sps_pps::parse_sps)
        nal_ref_idc != 0,
        pps.constrained_intra_pred_flag,
        pps.weighted_pred_flag,
        u16::try_from(pps.weighted_bipred_idc).unwrap_or(0),
        true, // MbsConsecutiveFlag (no FMO/ASO, enforced by h264_sps_pps::parse_pps)
        true, // frame_mbs_only_flag (enforced by h264_sps_pps::parse_sps)
        pps.transform_8x8_mode_flag,
        sps.direct_8x8_inference_flag, // MinLumaBipredSize8x8Flag <- direct_8x8_inference_flag
        is_intra_only,
    );

    DxvaPicParamsH264 {
        w_frame_width_in_mbs_minus1: u16::try_from(sps.mb_width.saturating_sub(1)).unwrap_or(0),
        w_frame_height_in_mbs_minus1: u16::try_from(sps.mb_height.saturating_sub(1)).unwrap_or(0),
        curr_pic: DxvaPicEntryH264::pack(u8::try_from(curr_pic_slot).unwrap_or(0), false),
        num_ref_frames: u8::try_from(sps.max_num_ref_frames).unwrap_or(16),
        w_bit_fields,
        bit_depth_luma_minus8: u8::try_from(sps.bit_depth_luma_minus8).unwrap_or(0),
        bit_depth_chroma_minus8: u8::try_from(sps.bit_depth_chroma_minus8).unwrap_or(0),
        reserved16_bits: 0,
        status_report_feedback_number,
        ref_frame_list,
        curr_field_order_cnt: [curr_top_poc, curr_bottom_poc],
        field_order_cnt_list,
        pic_init_qs_minus26: i8::try_from(pps.pic_init_qs_minus26).unwrap_or(0),
        chroma_qp_index_offset: i8::try_from(pps.chroma_qp_index_offset).unwrap_or(0),
        second_chroma_qp_index_offset: i8::try_from(pps.second_chroma_qp_index_offset).unwrap_or(0),
        continuation_flag: 1,
        pic_init_qp_minus26: i8::try_from(pps.pic_init_qp_minus26).unwrap_or(0),
        num_ref_idx_l0_active_minus1: u8::try_from(sh.num_ref_idx_l0_active_minus1).unwrap_or(0),
        num_ref_idx_l1_active_minus1: u8::try_from(sh.num_ref_idx_l1_active_minus1).unwrap_or(0),
        reserved8_bits_a: 0,
        frame_num_list,
        used_for_reference_flags,
        non_existing_frame_flags: 0,
        frame_num: u16::try_from(frame_num).unwrap_or(0),
        log2_max_frame_num_minus4: u8::try_from(sps.log2_max_frame_num.saturating_sub(4))
            .unwrap_or(0),
        pic_order_cnt_type: u8::try_from(sps.pic_order_cnt_type).unwrap_or(0),
        log2_max_pic_order_cnt_lsb_minus4: u8::try_from(
            sps.log2_max_pic_order_cnt_lsb.saturating_sub(4),
        )
        .unwrap_or(0),
        delta_pic_order_always_zero_flag: u8::from(sps.delta_pic_order_always_zero_flag),
        direct_8x8_inference_flag: u8::from(sps.direct_8x8_inference_flag),
        entropy_coding_mode_flag: u8::from(pps.entropy_coding_mode_flag),
        pic_order_present_flag: u8::from(pps.bottom_field_pic_order_in_frame_present_flag),
        num_slice_groups_minus1: u8::try_from(pps.num_slice_groups_minus1).unwrap_or(0),
        slice_group_map_type: 0,
        deblocking_filter_control_present_flag: u8::from(
            pps.deblocking_filter_control_present_flag,
        ),
        redundant_pic_cnt_present_flag: u8::from(pps.redundant_pic_cnt_present_flag),
        reserved8_bits_b: 0,
        slice_group_change_rate_minus1: 0,
        slice_group_map: [0u8; 810],
    }
}

/// Flat (unscaled, all-16) `DXVA_Qmatrix_H264` — see `h264_sps_pps`'s documented
/// scaling-list fidelity gap (custom scaling matrices are parsed for bit-sync but never
/// applied).
pub(super) const fn flat_qmatrix() -> DxvaQmatrixH264 {
    DxvaQmatrixH264 {
        scaling_lists_4x4: [[16u8; 16]; 6],
        scaling_lists_8x8: [[16u8; 64]; 2],
    }
}

const fn slice_type_dxva_value(slice_type: SliceType) -> u8 {
    match slice_type {
        SliceType::P => 0,
        SliceType::B => 1,
        SliceType::I => 2,
        SliceType::Sp => 3,
        SliceType::Si => 4,
    }
}

fn pack_ref_pic_list(entries: &[RefListEntry]) -> [DxvaPicEntryH264; 32] {
    let mut out = [DxvaPicEntryH264::UNUSED; 32];
    for (i, entry) in entries.iter().take(32).enumerate() {
        out[i] = DxvaPicEntryH264::pack(u8::try_from(entry.slot).unwrap_or(0), false);
    }
    out
}

/// Build `DXVA_Slice_H264_Long` for the current (sole) slice of this picture.
///
/// `bs_nal_unit_data_location`/`slice_bytes_in_buffer` describe the slice NAL's real
/// position in the compressed-bitstream input buffer (`ops.rs` owns that layout);
/// `bit_offset_to_slice_data` is the bit position, **relative to the start of the raw
/// NAL unit** (header byte included, `emulation_prevention_three_byte` bytes still
/// present — i.e. relative to `bs_nal_unit_data_location`, not to
/// `h264_slice::parse_slice_header`'s de-emulated RBSP), where `slice_data()` begins.
/// Callers must translate `parse_slice_header`'s returned de-emulated bit count via
/// [`super::h264_slice::rbsp_bit_offset_to_raw_bit_offset`] plus 8 (the header byte)
/// before calling this function — see `d3d12_video_decode.rs::decode_slice` and its
/// ADR-0002 Addendum note (getting this wrong hung a real GPU on real hardware).
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors DXVA_Slice_H264_Long's field list 1:1"
)]
pub(super) fn build_slice_long(
    sh: &SliceHeader,
    num_mbs_for_slice: u32,
    bit_offset_to_slice_data: u32,
    bs_nal_unit_data_location: u32,
    slice_bytes_in_buffer: u32,
    ref_list0: &[RefListEntry],
    ref_list1: &[RefListEntry],
) -> DxvaSliceH264Long {
    DxvaSliceH264Long {
        bs_nal_unit_data_location,
        slice_bytes_in_buffer,
        w_bad_slice_chopping: 0,
        first_mb_in_slice: 0,
        num_mbs_for_slice: u16::try_from(num_mbs_for_slice).unwrap_or(u16::MAX),
        bit_offset_to_slice_data: u16::try_from(bit_offset_to_slice_data).unwrap_or(u16::MAX),
        slice_type: slice_type_dxva_value(sh.slice_type),
        luma_log2_weight_denom: 0,
        chroma_log2_weight_denom: 0,
        num_ref_idx_l0_active_minus1: u8::try_from(sh.num_ref_idx_l0_active_minus1).unwrap_or(0),
        num_ref_idx_l1_active_minus1: u8::try_from(sh.num_ref_idx_l1_active_minus1).unwrap_or(0),
        slice_alpha_c0_offset_div2: i8::try_from(sh.slice_alpha_c0_offset_div2).unwrap_or(0),
        slice_beta_offset_div2: i8::try_from(sh.slice_beta_offset_div2).unwrap_or(0),
        reserved8_bits: 0,
        ref_pic_list: [pack_ref_pic_list(ref_list0), pack_ref_pic_list(ref_list1)],
        weights: [[[[0i16; 2]; 3]; 32]; 2],
        slice_qs_delta: 0,
        slice_qp_delta: i8::try_from(sh.slice_qp_delta).unwrap_or(0),
        redundant_pic_cnt: 0,
        direct_spatial_mv_pred_flag: u8::from(sh.direct_spatial_mv_pred_flag),
        cabac_init_idc: u8::try_from(sh.cabac_init_idc).unwrap_or(0),
        disable_deblocking_filter_idc: u8::try_from(sh.disable_deblocking_filter_idc).unwrap_or(0),
        slice_id: 0,
    }
}
