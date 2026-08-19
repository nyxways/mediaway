//! DXVA-shaped AV1 picture-parameter / tile-control structs and the logic that packs
//! parsed sequence-header/frame-header state into them.
//!
//! **Ground truth, cited (ADR-0005 § DXVA struct definitions)**: same absent-from-
//! `windows`-crate situation `h264_pic_params.rs`/`hevc_pic_params.rs` document for their
//! own codecs — `DXVA_PicParams_AV1`/`DXVA_PicEntry_AV1`/`DXVA_Tile_AV1` are absent from
//! the pinned `windows` crate's generated bindings entirely (grepped, zero matches; only
//! the `D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0`/etc. GUIDs are present). Hand-defined
//! here, `repr(C)`, ground-truthed against Microsoft's own official Windows Driver DDI
//! reference (`learn.microsoft.com/.../ns-dxva-dxva_picparams_av1`, fetched directly this
//! implementation pass, including the `cdef`/`segmentation`/`film_grain` sub-struct field
//! lists ADR-0005's own abridged reproduction omitted for length) — a **primary** source,
//! stronger footing than ADR-0002 (H.264, Wine mirror) or ADR-0004 (HEVC, Wine mirror plus
//! an acknowledged Microsoft Learn rendering discrepancy) had for their own codecs.
//!
//! **No `windows`-crate reference struct exists to compare against via `std::mem::size_of`**
//! (same situation `hevc_pic_params_tests.rs` documents) — this module's own struct-size
//! tests are self-consistency checks only, not ground-truthed against a second,
//! independent source.
//!
//! **`DXVA_PicParams_AV1` has no separate qmatrix struct/DXVA argument** — `qm_y`/`qm_u`/
//! `qm_v` are plain scalar fields inline in `quantization` (a real structural difference
//! from H.264/HEVC, not a gap — ADR-0005 § DXVA struct definitions). `av1_ops.rs` builds
//! only two `D3D12_VIDEO_DECODE_FRAME_ARGUMENT` entries (`PICTURE_PARAMETERS` +
//! `SLICE_CONTROL`), not three.

#![forbid(unsafe_code)]

use super::av1_frame_header::FrameHeader;
use super::av1_sequence_header::SequenceHeader;

/// `DXVA_PicEntry_AV1`. `0xFF` `index` marks an unused/invalid entry — this module's
/// `KEY_FRAME`-only scope never has a real reference, so `frame_refs[7]` is always this
/// constant (ADR-0005 § Context finding #4, § Decision's "no `av1_refs.rs`" note).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DxvaPicEntryAv1 {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) wmmat: [i32; 6],
    /// Packed `wminvalid:1 | wmtype:2 | Reserved:5`.
    pub(super) global_motion_flags: u8,
    /// Index into `RefFrameMapTextureIndex[]`; `0xFF` = invalid/unused.
    pub(super) index: u8,
    pub(super) reserved16_bits: u16,
}

impl DxvaPicEntryAv1 {
    pub(super) const UNUSED: Self = Self {
        width: 0,
        height: 0,
        wmmat: [0; 6],
        global_motion_flags: 0,
        index: 0xFF,
        reserved16_bits: 0,
    };
}

/// `DXVA_Tile_AV1`. `anchor_frame == 0xFF` when not part of a Tile List OBU — always the
/// case in this module's single-tile, non-tile-list scope.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct DxvaTileAv1 {
    pub(super) data_offset: u32,
    pub(super) data_size: u32,
    pub(super) row: u16,
    pub(super) column: u16,
    pub(super) reserved16_bits: u16,
    pub(super) anchor_frame: u8,
    pub(super) reserved8_bits: u8,
}

/// `DXVA_PicParams_AV1.tiles`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct DxvaTilesAv1 {
    pub(super) cols: u8,
    pub(super) rows: u8,
    pub(super) context_update_id: u16,
    pub(super) widths: [u16; 64],
    pub(super) heights: [u16; 64],
}

impl Default for DxvaTilesAv1 {
    fn default() -> Self {
        // [u16; 64] has no std `Default` impl (only array sizes <= 32 do) — an explicit
        // array literal sidesteps that without needing `Default` on the element count.
        Self {
            cols: 0,
            rows: 0,
            context_update_id: 0,
            widths: [0u16; 64],
            heights: [0u16; 64],
        }
    }
}

/// `DXVA_PicParams_AV1.loop_filter`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DxvaLoopFilterAv1 {
    pub(super) filter_level: [u8; 2],
    pub(super) filter_level_u: u8,
    pub(super) filter_level_v: u8,
    pub(super) sharpness_level: u8,
    /// Packed `mode_ref_delta_enabled:1 | mode_ref_delta_update:1 | delta_lf_multi:1 |
    /// delta_lf_present:1 | Reserved:4`.
    pub(super) control_flags: u8,
    pub(super) ref_deltas: [i8; 8],
    pub(super) mode_deltas: [i8; 2],
    pub(super) delta_lf_res: u8,
    pub(super) frame_restoration_type: [u8; 3],
    pub(super) log2_restoration_unit_size: [u16; 3],
    pub(super) reserved16_bits: u16,
}

/// `DXVA_PicParams_AV1.quantization`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DxvaQuantizationAv1 {
    /// Packed `delta_q_present:1 | delta_q_res:2 | Reserved:5`.
    pub(super) control_flags: u8,
    pub(super) base_qindex: u8,
    pub(super) y_dc_delta_q: i8,
    pub(super) u_dc_delta_q: i8,
    pub(super) v_dc_delta_q: i8,
    pub(super) u_ac_delta_q: i8,
    pub(super) v_ac_delta_q: i8,
    /// `0xFF` when `using_qmatrix == 0` (this module's scope, always — rejected if `1`).
    pub(super) qm_y: u8,
    pub(super) qm_u: u8,
    pub(super) qm_v: u8,
    pub(super) reserved16_bits: u16,
}

/// `DXVA_PicParams_AV1.cdef` — always all-zero in this module's scope (`enable_cdef`
/// rejected at the sequence-header level, `av1_frame_header.rs::parse_frame_header`'s
/// `cdef_params()` no-op).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DxvaCdefAv1 {
    /// Packed `damping:2 | bits:2 | Reserved:4`.
    pub(super) control_flags: u8,
    /// Each byte packed `primary:6 | secondary:2`.
    pub(super) y_strengths: [u8; 8],
    pub(super) uv_strengths: [u8; 8],
}

/// `DXVA_PicParams_AV1.segmentation` — always all-zero/disabled in this module's scope
/// (`segmentation_enabled` rejected, `av1_frame_header.rs`'s `parse_segmentation_params`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DxvaSegmentationAv1 {
    /// Packed `enabled:1 | update_map:1 | update_data:1 | temporal_update:1 | Reserved:4`.
    pub(super) control_flags: u8,
    pub(super) reserved24_bits: [u8; 3],
    /// Each byte packed `alt_q:1 | alt_lf_y_v:1 | alt_lf_y_h:1 | alt_lf_u:1 | alt_lf_v:1 |
    /// ref_frame:1 | skip:1 | globalmv:1`.
    pub(super) feature_mask: [u8; 8],
    pub(super) feature_data: [[i16; 8]; 8],
}

/// `DXVA_PicParams_AV1.film_grain` — always all-zero/disabled in this module's scope
/// (`film_grain_params_present` rejected, `av1_sequence_header.rs`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DxvaFilmGrainAv1 {
    pub(super) control_flags: u16,
    pub(super) grain_seed: u16,
    pub(super) scaling_points_y: [[u8; 2]; 14],
    pub(super) num_y_points: u8,
    pub(super) scaling_points_cb: [[u8; 2]; 10],
    pub(super) num_cb_points: u8,
    pub(super) scaling_points_cr: [[u8; 2]; 10],
    pub(super) num_cr_points: u8,
    pub(super) ar_coeffs_y: [u8; 24],
    pub(super) ar_coeffs_cb: [u8; 25],
    pub(super) ar_coeffs_cr: [u8; 25],
    pub(super) cb_mult: u8,
    pub(super) cb_luma_mult: u8,
    pub(super) cr_mult: u8,
    pub(super) cr_luma_mult: u8,
    pub(super) reserved8_bits: u8,
    pub(super) cb_offset: i16,
    pub(super) cr_offset: i16,
}

/// `DXVA_PicParams_AV1`, `repr(C)` field-for-field (see module doc for the primary-source
/// citation). The `union { struct { bitfields }; TYPE named; }` groups become plain
/// integers (Rust has no native C bitfields) — see [`pack_coding_param_tool_flags`]/
/// [`pack_format_and_picture_info_flags`] for each group's bit layout.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(super) struct DxvaPicParamsAv1 {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) max_width: u32,
    pub(super) max_height: u32,
    pub(super) curr_pic_texture_index: u8,
    pub(super) superres_denom: u8,
    pub(super) bitdepth: u8,
    pub(super) seq_profile: u8,
    pub(super) tiles: DxvaTilesAv1,
    pub(super) coding_param_tool_flags: u32,
    pub(super) format_and_picture_info_flags: u8,
    pub(super) primary_ref_frame: u8,
    pub(super) order_hint: u8,
    pub(super) order_hint_bits: u8,
    pub(super) frame_refs: [DxvaPicEntryAv1; 7],
    pub(super) ref_frame_map_texture_index: [u8; 8],
    pub(super) loop_filter: DxvaLoopFilterAv1,
    pub(super) quantization: DxvaQuantizationAv1,
    pub(super) cdef: DxvaCdefAv1,
    pub(super) interp_filter: u8,
    pub(super) segmentation: DxvaSegmentationAv1,
    pub(super) film_grain: DxvaFilmGrainAv1,
    pub(super) reserved32_bits: u32,
    pub(super) status_report_feedback_number: u32,
}

/// AV1 `SUPERRES_NUM` — `superres_denom` when `use_superres == 0` (this module's scope,
/// always, `enable_superres` rejected at the sequence header).
const SUPERRES_NUM: u8 = 8;
/// `PRIMARY_REF_NONE` (AV1 spec § "Symbols") — always this module's `primary_ref_frame`
/// value (`FrameIsIntra` always true, ADR-0005 § Scope decision).
const PRIMARY_REF_NONE: u8 = 7;
/// `TOTAL_REFS_PER_FRAME` (AV1 spec) — width of `RefFrameMapTextureIndex`/
/// `loop_filter.ref_deltas`.
const TOTAL_REFS_PER_FRAME: usize = 8;
/// `force_integer_mv` is spec-inferred `1` for every `KEY_FRAME` (`FrameIsIntra`) picture,
/// regardless of its own unread bitstream value — see
/// `av1_frame_header.rs::parse_frame_header`'s own comment.
const FORCE_INTEGER_MV_INTRA: bool = true;

/// Pack `coding.CodingParamToolFlags` (32 bits total; bits 27-31 `Reserved`, always `0`).
#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "mirrors the DXVA bitfield layout 1:1 — each bool is one independent \
    bitfield, not a state machine, same reasoning h264_pic_params.rs's pack_bit_fields gives"
)]
fn pack_coding_param_tool_flags(
    use_128x128_superblock: bool,
    intra_edge_filter: bool,
    interintra_compound: bool,
    masked_compound: bool,
    dual_filter: bool,
    jnt_comp: bool,
    disable_frame_end_update_cdf: bool,
    disable_cdf_update: bool,
    reduced_tx_set: bool,
    tx_mode: u8,
    enable_ref_frame_mvs: bool,
    filter_intra: bool,
) -> u32 {
    u32::from(use_128x128_superblock)
        | (u32::from(intra_edge_filter) << 1)
        | (u32::from(interintra_compound) << 2)
        | (u32::from(masked_compound) << 3)
        // warped_motion (coding.warped_motion == per-frame allow_warped_motion, always 0
        // in this module's scope — FrameIsIntra) — bit 4 left unset.
        | (u32::from(dual_filter) << 5)
        | (u32::from(jnt_comp) << 6)
        // screen_content_tools (allow_screen_content_tools, always 0) — bit 7 unset.
        | (u32::from(FORCE_INTEGER_MV_INTRA) << 8)
        // cdef / restoration / film_grain / intrabc / high_precision_mv: always 0 in this
        // module's scope (rejected upstream or FrameIsIntra-forced) — bits 9-13 unset.
        // switchable_motion_mode: no per-frame syntax for an intra-only frame — bit 14 unset.
        | (u32::from(filter_intra) << 15)
        | (u32::from(disable_frame_end_update_cdf) << 16)
        | (u32::from(disable_cdf_update) << 17)
        // reference_mode / skip_mode: always 0 (FrameIsIntra) — bits 18-19 unset.
        | (u32::from(reduced_tx_set) << 20)
        // superres: always 0 (enable_superres rejected) — bit 21 unset.
        | (u32::from(tx_mode & 0b11) << 22)
        // use_ref_frame_mvs: always 0 (FrameIsIntra) — bit 24 unset.
        | (u32::from(enable_ref_frame_mvs) << 25)
        | (1 << 26) // reference_frame_update: always 1 (show_existing_frame == 0 always)
}

/// Pack `format.FormatAndPictureInfoFlags` (8 bits total; bit 7 `Reserved`, always `0`).
/// This module's scope forces `frame_type == KEY_FRAME(0)`, `show_frame == 1`,
/// `showable_frame == 0` (`KEY_FRAME`), `subsampling_x == subsampling_y == 1` (4:2:0),
/// `mono_chrome == 0` — every one of these is a validated invariant by the time
/// [`build_pic_params`] is called, not a real function parameter.
const fn pack_format_and_picture_info_flags() -> u8 {
    0b0011_0100 // frame_type=00, show_frame=1(bit2), subsampling_x=1(bit4), subsampling_y=1(bit5)
}

/// Build `DXVA_PicParams_AV1` for the current picture. `output_slot` is this picture's DPB
/// slot (`CurrPicTextureIndex`); `frame_refs`/`RefFrameMapTextureIndex` are always the
/// all-`0xFF` empty state (ADR-0005 § Context finding #4 — no picture is ever referenced
/// under this module's `KEY_FRAME`-only scope).
#[allow(
    clippy::too_many_lines,
    reason = "one linear DXVA struct fill, mirrors hevc_pic_params.rs::build_pic_params's identical shape"
)]
pub(super) fn build_pic_params(
    seq: &SequenceHeader,
    fh: &FrameHeader,
    output_slot: u32,
    status_report_feedback_number: u32,
) -> DxvaPicParamsAv1 {
    let coding_param_tool_flags = pack_coding_param_tool_flags(
        seq.use_128x128_superblock,
        seq.enable_intra_edge_filter,
        seq.enable_interintra_compound,
        seq.enable_masked_compound,
        seq.enable_dual_filter,
        seq.enable_jnt_comp,
        fh.disable_frame_end_update_cdf,
        fh.disable_cdf_update,
        fh.reduced_tx_set,
        fh.tx_mode,
        seq.enable_ref_frame_mvs,
        seq.enable_filter_intra,
    );

    let loop_filter_control_flags = u8::from(fh.loop_filter.delta_enabled)
        | (u8::from(fh.loop_filter.delta_update) << 1)
        | (u8::from(fh.delta_lf_multi) << 2)
        | (u8::from(fh.delta_lf_present) << 3);
    let mut ref_deltas = [0i8; TOTAL_REFS_PER_FRAME];
    for (dst, &src) in ref_deltas.iter_mut().zip(fh.loop_filter.ref_deltas.iter()) {
        *dst = i8::try_from(src).unwrap_or(0);
    }
    let mut mode_deltas = [0i8; 2];
    for (dst, &src) in mode_deltas
        .iter_mut()
        .zip(fh.loop_filter.mode_deltas.iter())
    {
        *dst = i8::try_from(src).unwrap_or(0);
    }
    let loop_filter = DxvaLoopFilterAv1 {
        filter_level: [
            u8::try_from(fh.loop_filter.level[0]).unwrap_or(0),
            u8::try_from(fh.loop_filter.level[1]).unwrap_or(0),
        ],
        filter_level_u: u8::try_from(fh.loop_filter.level_u).unwrap_or(0),
        filter_level_v: u8::try_from(fh.loop_filter.level_v).unwrap_or(0),
        sharpness_level: u8::try_from(fh.loop_filter.sharpness).unwrap_or(0),
        control_flags: loop_filter_control_flags,
        ref_deltas,
        mode_deltas,
        delta_lf_res: u8::try_from(fh.delta_lf_res).unwrap_or(0),
        // No loop restoration in this module's scope (enable_restoration rejected at the
        // sequence header) — always RESTORE_NONE(0) for every plane.
        frame_restoration_type: [0u8; 3],
        log2_restoration_unit_size: [0u16; 3],
        reserved16_bits: 0,
    };

    let quantization_control_flags =
        u8::from(fh.delta_q_present) | (u8::try_from(fh.delta_q_res & 0b11).unwrap_or(0) << 1);
    let quantization = DxvaQuantizationAv1 {
        control_flags: quantization_control_flags,
        base_qindex: u8::try_from(fh.quantization.base_q_idx).unwrap_or(0),
        y_dc_delta_q: i8::try_from(fh.quantization.delta_q_y_dc).unwrap_or(0),
        u_dc_delta_q: i8::try_from(fh.quantization.delta_q_u_dc).unwrap_or(0),
        v_dc_delta_q: i8::try_from(fh.quantization.delta_q_v_dc).unwrap_or(0),
        u_ac_delta_q: i8::try_from(fh.quantization.delta_q_u_ac).unwrap_or(0),
        v_ac_delta_q: i8::try_from(fh.quantization.delta_q_v_ac).unwrap_or(0),
        // using_qmatrix is always 0 in this module's scope (rejected if 1) -> 0xFF
        // "invalid quantizer matrix level" per the DXVA field doc.
        qm_y: 0xFF,
        qm_u: 0xFF,
        qm_v: 0xFF,
        reserved16_bits: 0,
    };

    let mut tiles = DxvaTilesAv1 {
        cols: 1,
        rows: 1,
        context_update_id: 0,
        ..DxvaTilesAv1::default()
    };
    tiles.widths[0] = u16::try_from(fh.tile.tile_width_sb).unwrap_or(u16::MAX);
    tiles.heights[0] = u16::try_from(fh.tile.tile_height_sb).unwrap_or(u16::MAX);

    DxvaPicParamsAv1 {
        width: fh.width,
        height: fh.height,
        max_width: seq.max_frame_width,
        max_height: seq.max_frame_height,
        curr_pic_texture_index: u8::try_from(output_slot).unwrap_or(0),
        superres_denom: SUPERRES_NUM,
        bitdepth: 8,
        seq_profile: 0,
        tiles,
        coding_param_tool_flags,
        format_and_picture_info_flags: pack_format_and_picture_info_flags(),
        primary_ref_frame: PRIMARY_REF_NONE,
        order_hint: u8::try_from(fh.order_hint).unwrap_or(0),
        order_hint_bits: u8::try_from(seq.order_hint_bits).unwrap_or(0),
        // No reference-frame use of any kind under this module's scope — every entry
        // stays the unused sentinel (ADR-0005 § Context finding #4).
        frame_refs: [DxvaPicEntryAv1::UNUSED; 7],
        ref_frame_map_texture_index: [0xFFu8; TOTAL_REFS_PER_FRAME],
        loop_filter,
        quantization,
        // No CDEF in this module's scope (enable_cdef rejected) — always all-zero.
        cdef: DxvaCdefAv1::default(),
        // No per-frame interpolation-filter syntax for an intra-only frame (AV1 spec —
        // read_interpolation_filter() is only reached on the non-FrameIsIntra branch of
        // uncompressed_header()) — 0 ("normal 8-tap") is a harmless placeholder, matching
        // this backend's own encoder default (D3D12_VIDEO_ENCODER_AV1_INTERPOLATION_
        // FILTERS_EIGHTTAP == 0).
        interp_filter: 0,
        // No segmentation in this module's scope (segmentation_enabled rejected) — always
        // all-zero/disabled.
        segmentation: DxvaSegmentationAv1::default(),
        // No film grain in this module's scope (film_grain_params_present rejected) —
        // always all-zero/disabled.
        film_grain: DxvaFilmGrainAv1::default(),
        reserved32_bits: 0,
        status_report_feedback_number,
    }
}

/// Build the sole `DXVA_Tile_AV1` entry for this picture's one tile (this module's
/// single-tile scope, ADR-0005 § Scope decision).
pub(super) const fn build_tile(data_offset: u32, data_size: u32) -> DxvaTileAv1 {
    DxvaTileAv1 {
        data_offset,
        data_size,
        row: 0,
        column: 0,
        reserved16_bits: 0,
        // Not part of a Tile List OBU (this module never emits/consumes one) — 0xFF per
        // the DXVA_Tile_AV1 field doc.
        anchor_frame: 0xFF,
        reserved8_bits: 0,
    }
}

#[cfg(test)]
#[path = "av1_pic_params_tests.rs"]
mod tests;
