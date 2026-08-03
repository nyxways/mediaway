//! Minimal AV1 OBU (temporal delimiter / sequence header / frame header) writer for the
//! D3D12 native video-encode backend.
//!
//! Unlike H.264 Annex-B / HEVC Annex-B (NAL start-code framing, no explicit length field —
//! see [`super::bitstream`]/[`super::bitstream_hevc`]), AV1 uses length-prefixed OBUs
//! (`leb128`-coded `obu_size`); no emulation prevention is needed. This backend writes a
//! **fixed session prefix** (temporal delimiter + sequence header OBUs, byte-identical
//! every packet — mirrors the H.264/HEVC SPS/PPS-every-packet pattern, see
//! [`build_av1_session_prefix`]) plus a **fixed frame-header OBU payload**
//! ([`build_av1_frame_header_bytes`], also byte-identical every packet in this backend's
//! all-fixed-config design — every field below is a hardcoded constant, never varies frame
//! to frame). Only the driver's compressed tile bytes and the `OBU_FRAME`'s `leb128`
//! `obu_size` (which depends on the driver's actual per-frame compressed byte count) vary
//! — built per packet in [`super::ops_av1::D3d12VideoEncoder::read_packet_av1`].
//!
//! Scope matches this backend's all-intra/no-reference/single-tile/no-CDEF/no-restoration
//! configuration: Main profile (`seq_profile == 0`), 8-bit 4:2:0 (matches NV12),
//! `reduced_still_picture_header == 0` (a real multi-frame stream — every frame
//! independently a key frame, not AV1's single-frame "still picture" mode),
//! `enable_order_hint == 0` (drops `OrderHint`/ref-order-hint signaling entirely), one
//! tile. Field values ground-truthed against the AV1 Bitstream & Decoding Process
//! Specification v1.0.0 §5.5 (`sequence_header_obu`), §5.5.2 (`color_config`), §5.9.2
//! (`uncompressed_header`), §5.9.5 (`frame_size`/`render_size`/`superres_params`), §5.9.12
//! (`quantization_params`), §5.9.15 (`tile_info`), §5.9.11 (`loop_filter_params`), §5.9.19
//! (`cdef_params`), §5.9.20 (`lr_params`).

#![forbid(unsafe_code)]

use super::bitstream::RbspWriter;

const OBU_SEQUENCE_HEADER: u8 = 1;
const OBU_TEMPORAL_DELIMITER: u8 = 2;
pub(super) const OBU_FRAME: u8 = 6;

/// AV1 Main profile `seq_profile` / `D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN` (8-bit 4:2:0,
/// matches NV12).
const SEQ_PROFILE_MAIN: u32 = 0;

/// `leb128()` (AV1 spec §4.10.5): little-endian base-128 with a continuation bit.
pub(super) fn write_leb128(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        byte |= 0x80;
        out.push(byte);
    }
}

/// `obu_header()` (AV1 spec §5.3.2) for a non-extended OBU with `obu_has_size_field == 1`.
pub(super) const fn obu_header_byte(obu_type: u8) -> u8 {
    (obu_type << 3) | 0b10
}

/// Wrap `payload` as a complete OBU: header byte + `leb128(payload.len())` + `payload`.
fn wrap_obu(obu_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 3);
    out.push(obu_header_byte(obu_type));
    write_leb128(&mut out, payload.len() as u64);
    out.extend_from_slice(payload);
    out
}

const fn bit_length(value: u32) -> u32 {
    32 - value.leading_zeros()
}

const fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

/// `tile_info()` (AV1 spec §5.9.15) for this backend's always-minimum tile-column/row
/// count (`uniform_tile_spacing_flag == 1`, always signal "stop incrementing" immediately
/// at the minimum log2 tile count this resolution's driver-imposed `MAX_TILE_WIDTH`/
/// `MAX_TILE_AREA` limits allow). For every resolution this backend validates (well under
/// the ~4096px-wide limit that would force `TileColsLog2`/`TileRowsLog2 > 0`), this always
/// resolves to exactly one tile column and one tile row (`NumTiles == 1`), matching
/// [`super::ops_av1`]'s single-tile `tile_group_obu()` framing (no explicit `tile_size`
/// field — the one tile is always `tg_end == NumTiles - 1`).
fn write_tile_info(w: &mut RbspWriter, mi_cols: u32, mi_rows: u32) {
    const MAX_TILE_WIDTH: u32 = 4096;
    const MAX_TILE_AREA: u32 = 4096 * 2304;
    const MAX_TILE_COLS: u32 = 64;
    const MAX_TILE_ROWS: u32 = 64;
    const SB_SHIFT: u32 = 4; // use_128x128_superblock == 0 -> 64x64 superblocks
    const SB_SIZE: u32 = SB_SHIFT + 2;

    let sb_cols = (mi_cols + 15) >> SB_SHIFT;
    let sb_rows = (mi_rows + 15) >> SB_SHIFT;
    let max_tile_width_sb = MAX_TILE_WIDTH >> SB_SIZE;
    let max_tile_area_sb = MAX_TILE_AREA >> (2 * SB_SIZE);
    let min_log2_tile_cols = tile_log2(max_tile_width_sb, sb_cols);
    let max_log2_tile_cols = tile_log2(1, sb_cols.min(MAX_TILE_COLS));
    let max_log2_tile_rows = tile_log2(1, sb_rows.min(MAX_TILE_ROWS));
    let min_log2_tiles = min_log2_tile_cols.max(tile_log2(max_tile_area_sb, sb_rows * sb_cols));

    w.write_bit(1); // uniform_tile_spacing_flag

    let tile_cols_log2 = min_log2_tile_cols;
    if tile_cols_log2 < max_log2_tile_cols {
        w.write_bit(0); // increment_tile_cols_log2 = 0 -> stop at the minimum
    }

    let min_log2_tile_rows = min_log2_tiles.saturating_sub(tile_cols_log2);
    let tile_rows_log2 = min_log2_tile_rows;
    if tile_rows_log2 < max_log2_tile_rows {
        w.write_bit(0); // increment_tile_rows_log2 = 0 -> stop at the minimum
    }

    if tile_cols_log2 > 0 || tile_rows_log2 > 0 {
        // Out of this backend's validated resolution range (would need >1 tile). Kept
        // spec-correct rather than omitted: context_update_tile_id (single tile -> 0) and
        // tile_size_bytes_minus_1 (never actually consumed — this backend's one tile is
        // always the last, which never carries an explicit tile_size field).
        w.write_bits(
            0,
            u8::try_from(tile_cols_log2 + tile_rows_log2).unwrap_or(31),
        );
        w.write_bits(0, 2);
    }
}

/// `sequence_header_obu()` (AV1 spec §5.5.1 + §5.5.2 `color_config`) for this backend's
/// fixed configuration.
fn write_sequence_header(
    w: &mut RbspWriter,
    width: u32,
    height: u32,
    seq_level_idx: u8,
    seq_tier: u8,
) {
    w.write_bits(SEQ_PROFILE_MAIN, 3); // seq_profile
    w.write_bit(0); // still_picture
    w.write_bit(0); // reduced_still_picture_header
    w.write_bit(0); // timing_info_present_flag
    w.write_bit(0); // initial_display_delay_present_flag
    w.write_bits(0, 5); // operating_points_cnt_minus_1 == 0
    w.write_bits(0, 12); // operating_point_idc[0]
    w.write_bits(u32::from(seq_level_idx), 5); // seq_level_idx[0]
    if seq_level_idx > 7 {
        w.write_bit(seq_tier); // seq_tier[0]
    }
    // decoder_model_info_present_flag == 0, initial_display_delay_present_flag == 0:
    // neither per-operating-point field is read.

    let width_bits = bit_length(width - 1).max(1);
    let height_bits = bit_length(height - 1).max(1);
    w.write_bits(width_bits - 1, 4); // frame_width_bits_minus_1
    w.write_bits(height_bits - 1, 4); // frame_height_bits_minus_1
    w.write_bits(width - 1, u8::try_from(width_bits).unwrap_or(16)); // max_frame_width_minus_1
    w.write_bits(height - 1, u8::try_from(height_bits).unwrap_or(16)); // max_frame_height_minus_1

    w.write_bit(0); // frame_id_numbers_present_flag
    w.write_bit(0); // use_128x128_superblock
    w.write_bit(0); // enable_filter_intra
    w.write_bit(0); // enable_intra_edge_filter
    w.write_bit(0); // enable_interintra_compound
    w.write_bit(0); // enable_masked_compound
    w.write_bit(0); // enable_warped_motion
    w.write_bit(0); // enable_dual_filter
    w.write_bit(0); // enable_order_hint
    // enable_order_hint == 0: enable_jnt_comp / enable_ref_frame_mvs not read.
    w.write_bit(0); // seq_choose_screen_content_tools
    w.write_bit(0); // seq_force_screen_content_tools (SELECT not chosen)
    // seq_force_screen_content_tools == 0: seq_choose_integer_mv / seq_force_integer_mv not read.
    // enable_order_hint == 0: order_hint_bits_minus_1 not read (OrderHintBits == 0).

    w.write_bit(0); // enable_superres
    w.write_bit(0); // enable_cdef
    w.write_bit(0); // enable_restoration

    // color_config(): 8-bit 4:2:0, matches NV12.
    w.write_bit(0); // high_bitdepth -> BitDepth == 8
    w.write_bit(0); // mono_chrome (seq_profile != 1, so read) -> NumPlanes == 3
    w.write_bit(0); // color_description_present_flag -> CP/TC/MC == UNSPECIFIED(2)
    // mono_chrome == 0 and (CP,TC,MC) != (BT_709,SRGB,IDENTITY):
    w.write_bit(0); // color_range
    // seq_profile == 0 -> subsampling_x = subsampling_y = 1 (4:2:0), not read.
    w.write_bits(0, 2); // chroma_sample_position (CSP_UNKNOWN) — read since subsampling_x && subsampling_y
    w.write_bit(0); // separate_uv_delta_q

    w.write_bit(0); // film_grain_params_present

    w.rbsp_trailing_bits(); // this OBU's own trailing_bits() (AV1 spec §5.3.1 general obu())
}

/// `uncompressed_header()` (AV1 spec §5.9.2) for this backend's fixed all-intra
/// configuration: every packet is an independent `KEY_FRAME` with `show_frame == 1`
/// (hence `error_resilient_mode == 1` and `refresh_frame_flags == allFrames` are both
/// spec-inferred, no bits read), `disable_cdf_update == 1`, `frame_size_override_flag ==
/// 0` (uses the sequence header's fixed resolution), single tile, no segmentation/CDEF/
/// restoration/delta-Q/delta-LF (all disabled in the sequence header, so their bitstream
/// fields read/write zero bits — see the module doc's spec-section list), `TxMode ==
/// TX_MODE_LARGEST`, `reduced_tx_set == 0`. Every field written here must match the
/// `D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_CODEC_DATA` this backend passes to
/// `EncodeFrame` for the same frame — see [`super::ops_av1`].
fn write_frame_header(w: &mut RbspWriter, base_q_idx: u8, width: u32, height: u32) {
    w.write_bit(0); // show_existing_frame
    w.write_bits(0, 2); // frame_type == KEY_FRAME(0)
    w.write_bit(1); // show_frame
    // show_frame == 1 -> showable_frame inferred 0 (KEY_FRAME), not read.
    // frame_type == KEY_FRAME && show_frame -> error_resilient_mode inferred 1, not read.

    w.write_bit(1); // disable_cdf_update
    // seq_force_screen_content_tools == 0 (not SELECT) -> allow_screen_content_tools
    // inferred 0, not read; force_integer_mv inferred 0 then FrameIsIntra overrides to 1,
    // neither read.
    // frame_id_numbers_present_flag == 0 -> current_frame_id not read.

    w.write_bit(0); // frame_size_override_flag
    // OrderHintBits == 0 -> order_hint f(0) reads nothing.
    // FrameIsIntra -> primary_ref_frame inferred PRIMARY_REF_NONE, not read.
    // decoder_model_info_present_flag == 0 -> no buffer-removal-time fields.
    // frame_type == KEY_FRAME && show_frame -> refresh_frame_flags inferred allFrames, not read.
    // FrameIsIntra && refresh_frame_flags == allFrames -> ref_order_hint loop skipped.

    // frame_size(): frame_size_override_flag == 0 -> width/height come from the sequence
    // header's max_frame_width/height_minus_1, no bits read here.
    // superres_params(): enable_superres == 0 -> use_superres inferred 0, SuperresDenom ==
    // SUPERRES_NUM (8) == D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_CODEC_DATA::SuperResDenominator.
    w.write_bit(0); // render_and_frame_size_different (render_size())
    // allow_screen_content_tools == 0 -> allow_intrabc not read.

    // disable_cdf_update == 1 -> disable_frame_end_update_cdf inferred 1, not read.
    // primary_ref_frame == PRIMARY_REF_NONE -> init_non_coeff_cdfs()/setup_past_independence(),
    // no bits. use_ref_frame_mvs == 0 -> motion_field_estimation() skipped.

    let mi_cols = 2 * ((width + 7) >> 3);
    let mi_rows = 2 * ((height + 7) >> 3);
    write_tile_info(w, mi_cols, mi_rows);

    // quantization_params()
    w.write_u8(base_q_idx); // base_q_idx
    w.write_bit(0); // DeltaQYDc: delta_coded
    // NumPlanes == 3 > 1, separate_uv_delta_q == 0 -> diff_uv_delta inferred 0, not read.
    w.write_bit(0); // DeltaQUDc: delta_coded
    w.write_bit(0); // DeltaQUAc: delta_coded
    // diff_uv_delta == 0 -> DeltaQVDc/DeltaQVAc copied from U, not read.
    w.write_bit(0); // using_qmatrix

    // segmentation_params()
    w.write_bit(0); // segmentation_enabled

    // delta_q_params(): base_q_idx > 0 -> delta_q_present is read.
    w.write_bit(0); // delta_q_present
    // delta_q_present == 0 -> delta_q_res not read, delta_lf_params() entirely skipped
    // (delta_lf_present inferred 0).

    // primary_ref_frame == PRIMARY_REF_NONE -> init_coeff_cdfs(), no bits.
    // CodedLossless == 0 (base_q_idx > 0) -> AllLossless == 0.

    // loop_filter_params(): not lossless, not intrabc -> fields are read.
    w.write_bits(0, 6); // loop_filter_level[0]
    w.write_bits(0, 6); // loop_filter_level[1]
    // NumPlanes > 1 but both levels == 0 -> loop_filter_level[2]/[3] not read.
    w.write_bits(0, 3); // loop_filter_sharpness
    w.write_bit(0); // loop_filter_delta_enabled

    // cdef_params(): enable_cdef == 0 -> entire function reads zero bits.
    // lr_params(): enable_restoration == 0 -> entire function reads zero bits.

    // read_tx_mode(): CodedLossless == 0 -> tx_mode_select is read.
    w.write_bit(0); // tx_mode_select -> TxMode == TX_MODE_LARGEST

    // frame_reference_mode(): FrameIsIntra -> reference_select inferred 0, not read.
    // skip_mode_params(): FrameIsIntra -> skip_mode_present inferred 0, not read.
    // allow_warped_motion: FrameIsIntra -> inferred 0, not read.

    w.write_bit(0); // reduced_tx_set

    // global_motion_params(): FrameIsIntra -> returns immediately, no bits.
    // film_grain_params(): film_grain_params_present == 0 -> returns immediately, no bits.

    w.byte_align_zero(); // frame_obu()'s byte_alignment() between frame header and tile group
}

/// Build the fixed temporal-delimiter + sequence-header OBU byte sequence for one AV1
/// encode session. `seq_level_idx`/`seq_tier` come from the driver's
/// `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` `SuggestedLevel` (see
/// [`super::av1::check_encoder_support`]) — `D3D12_VIDEO_ENCODER_AV1_LEVELS`' ordinal
/// values already equal the AV1 spec's `seq_level_idx` table (Annex A), unlike H.264/HEVC's
/// `level_idc`, so no lookup table is needed.
pub(super) fn build_av1_session_prefix(
    width: u32,
    height: u32,
    seq_level_idx: u8,
    seq_tier: u8,
) -> Vec<u8> {
    let mut seq_w = RbspWriter::new();
    write_sequence_header(&mut seq_w, width, height, seq_level_idx, seq_tier);
    let seq_header_bytes = seq_w.finish();

    let mut out = wrap_obu(OBU_TEMPORAL_DELIMITER, &[]);
    out.extend(wrap_obu(OBU_SEQUENCE_HEADER, &seq_header_bytes));
    out
}

/// Build this backend's fixed, byte-aligned `uncompressed_header()` bytes — identical
/// every frame (every field is a hardcoded constant, see [`write_frame_header`]). Not
/// itself OBU-wrapped: the caller ([`super::ops_av1::D3d12VideoEncoder::read_packet_av1`])
/// wraps it in `OBU_FRAME` together with the driver's per-frame compressed tile bytes,
/// since the OBU's `leb128` size depends on the driver's actual output length.
pub(super) fn build_av1_frame_header_bytes(base_q_idx: u8, width: u32, height: u32) -> Vec<u8> {
    let mut w = RbspWriter::new();
    write_frame_header(&mut w, base_q_idx, width, height);
    w.finish()
}
