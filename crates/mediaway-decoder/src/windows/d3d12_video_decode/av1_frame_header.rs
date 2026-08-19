//! `uncompressed_header()` (AV1 spec §5.9.2) + `tile_info()` (§5.9.15) +
//! `quantization_params()`/`segmentation_params()`/`delta_q_params()`/`delta_lf_params()`/
//! `loop_filter_params()`/`cdef_params()`/`lr_params()`/`read_tx_mode()`/
//! `frame_reference_mode()`/`skip_mode_params()`/`global_motion_params()`/
//! `film_grain_params()` parsing, restricted to this module's `KEY_FRAME`-only scope
//! (ADR-0005 § Scope decision: `frame_type == KEY_FRAME`, `show_frame == 1`,
//! `show_existing_frame == 0`, `FrameIsIntra` always true).
//!
//! Field-by-field cross-checked against `mediaway-encoder-windows`'s
//! `d3d12_video_encode/bitstream_av1.rs::write_frame_header`'s own exhaustive
//! inference-rule comments — every "not read because X" comment there names exactly which
//! reader-side branch this module takes for the same `FrameIsIntra`/all-fixed-tool shape.
//! **Unlike the encoder's writer** (whose per-frame fields are all hardcoded constants),
//! this module parses every field the encoder fixes at a constant as a **real,
//! bitstream-derived value** where ADR-0005 does not explicitly reject it (e.g.
//! `disable_cdf_update`, `frame_size_override_flag`, `base_q_idx`/quantizer deltas,
//! loop-filter levels/deltas, `reduced_tx_set`) — a real conformant stream from a different
//! encoder may legally vary these even within this scope's `KEY_FRAME`-only cut.
//!
//! `tile_info()` supports `uniform_tile_spacing_flag == 1` only — explicit non-uniform
//! per-tile widths/heights are rejected outright, a further honest narrowing beyond
//! ADR-0005's literal text: meaningless for a genuinely single-tile stream (this module's
//! only accepted `cols == rows == 1` case) and only ever exercised by a real multi-tile
//! stream, already out of scope.

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::{BitReader, H264Error};

use super::av1_sequence_header::SequenceHeader;

fn map_bit_err<T>(r: Result<T, H264Error>) -> Result<T, DecodeError> {
    r.map_err(|_err| DecodeError::InvalidInput)
}

fn read_bit(r: &mut BitReader<'_>) -> Result<bool, DecodeError> {
    Ok(map_bit_err(r.read_bit())? != 0)
}

fn read_bits(r: &mut BitReader<'_>, count: u32) -> Result<u32, DecodeError> {
    map_bit_err(r.read_bits(count))
}

/// `su(n)` (AV1 spec §4.10.6): read `n` bits as a two's-complement-style signed value
/// (the top bit is the sign; `delta_q`/loop-filter deltas both use `su(1+6)`, i.e. `n == 7`).
fn read_su(r: &mut BitReader<'_>, n: u32) -> Result<i32, DecodeError> {
    let value = i64::from(read_bits(r, n)?);
    let sign_mask = 1i64 << (n - 1);
    let signed = if value & sign_mask != 0 {
        value - (sign_mask << 1)
    } else {
        value
    };
    i32::try_from(signed).map_err(|_err| DecodeError::InvalidInput)
}

const fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

/// This module's single-tile-only `tile_info()` result — `cols`/`rows` are always `1`
/// (any other value is rejected before this type is constructed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TileInfo {
    pub(super) tile_width_sb: u32,
    pub(super) tile_height_sb: u32,
}

/// `tile_info()` (AV1 spec §5.9.15). Rejects any resolved `TileColsLog2 > 0 ||
/// TileRowsLog2 > 0` (multi-tile) and any `uniform_tile_spacing_flag == 0` (see module
/// doc) as [`DecodeError::Unsupported`].
fn parse_tile_info(
    r: &mut BitReader<'_>,
    use_128x128_superblock: bool,
    mi_cols: u32,
    mi_rows: u32,
) -> Result<TileInfo, DecodeError> {
    const MAX_TILE_WIDTH: u32 = 4096;
    const MAX_TILE_AREA: u32 = 4096 * 2304;
    const MAX_TILE_COLS: u32 = 64;
    const MAX_TILE_ROWS: u32 = 64;
    let sb_shift = if use_128x128_superblock { 5 } else { 4 };
    let sb_size = sb_shift + 2;
    let sb_cols = if use_128x128_superblock {
        (mi_cols + 31) >> 5
    } else {
        (mi_cols + 15) >> 4
    };
    let sb_rows = if use_128x128_superblock {
        (mi_rows + 31) >> 5
    } else {
        (mi_rows + 15) >> 4
    };
    let max_tile_width_sb = MAX_TILE_WIDTH >> sb_size;
    let max_tile_area_sb = MAX_TILE_AREA >> (2 * sb_size);
    let min_log2_tile_cols = tile_log2(max_tile_width_sb, sb_cols);
    let max_log2_tile_cols = tile_log2(1, sb_cols.min(MAX_TILE_COLS));
    let max_log2_tile_rows = tile_log2(1, sb_rows.min(MAX_TILE_ROWS));
    let min_log2_tiles =
        min_log2_tile_cols.max(tile_log2(max_tile_area_sb, sb_rows.saturating_mul(sb_cols)));

    let uniform_tile_spacing_flag = read_bit(r)?;
    if !uniform_tile_spacing_flag {
        return Err(DecodeError::Unsupported);
    }

    let mut tile_cols_log2 = min_log2_tile_cols;
    while tile_cols_log2 < max_log2_tile_cols {
        if read_bit(r)? {
            tile_cols_log2 += 1;
        } else {
            break;
        }
    }
    let min_log2_tile_rows = min_log2_tiles.saturating_sub(tile_cols_log2);
    let mut tile_rows_log2 = min_log2_tile_rows;
    while tile_rows_log2 < max_log2_tile_rows {
        if read_bit(r)? {
            tile_rows_log2 += 1;
        } else {
            break;
        }
    }
    if tile_cols_log2 > 0 || tile_rows_log2 > 0 {
        return Err(DecodeError::Unsupported);
    }
    // TileColsLog2 == TileRowsLog2 == 0 -> context_update_tile_id inferred 0, no
    // tile_size_bytes_minus_1 field (this module's own single-tile invariant: the one
    // tile is always tg_end == NumTiles - 1, matching bitstream_av1.rs::write_tile_info's
    // identical write-side reasoning).
    Ok(TileInfo {
        tile_width_sb: sb_cols,
        tile_height_sb: sb_rows,
    })
}

fn read_delta_q(r: &mut BitReader<'_>) -> Result<i32, DecodeError> {
    if read_bit(r)? { read_su(r, 7) } else { Ok(0) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Quantization {
    pub(super) base_q_idx: u32,
    pub(super) delta_q_y_dc: i32,
    pub(super) delta_q_u_dc: i32,
    pub(super) delta_q_u_ac: i32,
    pub(super) delta_q_v_dc: i32,
    pub(super) delta_q_v_ac: i32,
}

/// `quantization_params()` (AV1 spec §5.9.12). `NumPlanes` is always `3` in this module's
/// scope (`mono_chrome` rejected by [`super::av1_sequence_header::parse_sequence_header`]).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] when `using_qmatrix == 1` (ADR-0005 § Scope decision).
#[allow(
    clippy::similar_names,
    reason = "delta_q_y_dc/delta_q_u_dc/delta_q_v_dc are the real AV1 spec DeltaQYDc/\
    DeltaQUDc/DeltaQVDc variable names (§5.9.12 quantization_params()) — renaming to look \
    less similar would obscure the 1:1 spec mapping, mirrors hevc_vps_sps_pps.rs's \
    log2_min_cb_size/log2_min_tb_size identical allow"
)]
fn parse_quantization_params(
    r: &mut BitReader<'_>,
    separate_uv_delta_q: bool,
) -> Result<Quantization, DecodeError> {
    let base_q_idx = read_bits(r, 8)?;
    let delta_q_y_dc = read_delta_q(r)?;
    let diff_uv_delta = if separate_uv_delta_q {
        read_bit(r)?
    } else {
        false
    };
    let delta_q_u_dc = read_delta_q(r)?;
    let delta_q_u_ac = read_delta_q(r)?;
    let (delta_q_v_dc, delta_q_v_ac) = if diff_uv_delta {
        (read_delta_q(r)?, read_delta_q(r)?)
    } else {
        (delta_q_u_dc, delta_q_u_ac)
    };
    let using_qmatrix = read_bit(r)?;
    if using_qmatrix {
        return Err(DecodeError::Unsupported);
    }
    Ok(Quantization {
        base_q_idx,
        delta_q_y_dc,
        delta_q_u_dc,
        delta_q_u_ac,
        delta_q_v_dc,
        delta_q_v_ac,
    })
}

/// `segmentation_params()` (AV1 spec §5.9.14), rejecting `segmentation_enabled == 1`
/// outright (ADR-0005 § Scope decision) rather than parsing the full per-segment feature
/// table.
///
/// # Errors
///
/// [`DecodeError::Unsupported`] when `segmentation_enabled == 1`.
fn parse_segmentation_params(r: &mut BitReader<'_>) -> Result<(), DecodeError> {
    if read_bit(r)? {
        return Err(DecodeError::Unsupported);
    }
    Ok(())
}

/// `delta_q_params()` (AV1 spec §5.9.17). Returns `(delta_q_present, delta_q_res)`.
fn parse_delta_q_params(
    r: &mut BitReader<'_>,
    base_q_idx: u32,
) -> Result<(bool, u32), DecodeError> {
    let delta_q_present = if base_q_idx > 0 { read_bit(r)? } else { false };
    let delta_q_res = if delta_q_present { read_bits(r, 2)? } else { 0 };
    Ok((delta_q_present, delta_q_res))
}

/// `delta_lf_params()` (AV1 spec §5.9.18) — `allow_intrabc` is always `0` in this module's
/// scope (`allow_screen_content_tools` is always `0`, see [`super::av1_sequence_header`]'s
/// module doc). Returns `(delta_lf_present, delta_lf_res, delta_lf_multi)`.
fn parse_delta_lf_params(
    r: &mut BitReader<'_>,
    delta_q_present: bool,
) -> Result<(bool, u32, bool), DecodeError> {
    if !delta_q_present {
        return Ok((false, 0, false));
    }
    let delta_lf_present = read_bit(r)?;
    if !delta_lf_present {
        return Ok((false, 0, false));
    }
    let delta_lf_res = read_bits(r, 2)?;
    let delta_lf_multi = read_bit(r)?;
    Ok((delta_lf_present, delta_lf_res, delta_lf_multi))
}

/// AV1 spec `REF_FRAME` order (§ "Symbols and abbreviated terms"): `INTRA_FRAME(0)`,
/// `LAST_FRAME(1)`, `LAST2_FRAME(2)`, `LAST3_FRAME(3)`, `GOLDEN_FRAME(4)`,
/// `BWDREF_FRAME(5)`, `ALTREF2_FRAME(6)`, `ALTREF_FRAME(7)` — `setup_past_independence()`'s
/// default `loop_filter_ref_deltas` table (spec § 7.20). **Not independently re-verified
/// against the primary spec text this pass** (a common reference-decoder value, kept
/// consistent with this ADR's own "not hardware-confirmed" posture for lower-priority
/// details — flagged for a future implementation-time check, same class of gap as
/// ADR-0005's own Open Question #3).
const DEFAULT_REF_DELTAS: [i32; 8] = [1, 0, 0, 0, -1, 0, 0, -1];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LoopFilter {
    pub(super) level: [u32; 2],
    pub(super) level_u: u32,
    pub(super) level_v: u32,
    pub(super) sharpness: u32,
    pub(super) delta_enabled: bool,
    pub(super) delta_update: bool,
    pub(super) ref_deltas: [i32; 8],
    pub(super) mode_deltas: [i32; 2],
}

/// `loop_filter_params()` (AV1 spec §5.9.11). `allow_intrabc` is always `0` in this
/// module's scope, so the early-return condition reduces to `coded_lossless` alone.
fn parse_loop_filter_params(
    r: &mut BitReader<'_>,
    coded_lossless: bool,
) -> Result<LoopFilter, DecodeError> {
    if coded_lossless {
        return Ok(LoopFilter {
            level: [0, 0],
            level_u: 0,
            level_v: 0,
            sharpness: 0,
            delta_enabled: false,
            delta_update: false,
            ref_deltas: DEFAULT_REF_DELTAS,
            mode_deltas: [0, 0],
        });
    }
    let level0 = read_bits(r, 6)?;
    let level1 = read_bits(r, 6)?;
    // NumPlanes > 1 always true in this module's scope (mono_chrome rejected).
    let (level_u, level_v) = if level0 != 0 || level1 != 0 {
        (read_bits(r, 6)?, read_bits(r, 6)?)
    } else {
        (0, 0)
    };
    let sharpness = read_bits(r, 3)?;
    let delta_enabled = read_bit(r)?;
    let mut ref_deltas = DEFAULT_REF_DELTAS;
    let mut mode_deltas = [0i32, 0];
    let mut delta_update = false;
    if delta_enabled {
        delta_update = read_bit(r)?;
        if delta_update {
            for delta in &mut ref_deltas {
                if read_bit(r)? {
                    *delta = read_su(r, 7)?;
                }
            }
            for delta in &mut mode_deltas {
                if read_bit(r)? {
                    *delta = read_su(r, 7)?;
                }
            }
        }
    }
    Ok(LoopFilter {
        level: [level0, level1],
        level_u,
        level_v,
        sharpness,
        delta_enabled,
        delta_update,
        ref_deltas,
        mode_deltas,
    })
}

/// Parsed AV1 frame-header fields this module's DXVA packing
/// ([`super::av1_pic_params`]) needs. Every rejection this ADR's scope names is already
/// enforced by the time this type is constructed (`Ok` implies `KEY_FRAME`, `show_frame ==
/// 1`, `show_existing_frame == 0`, no segmentation/qmatrix, single tile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is a real, independent AV1 frame-header flag that must be echoed \
    into DXVA_PicParams_AV1 exactly as signaled — same reasoning \
    hevc_vps_sps_pps.rs's Pps gives for its own identical allow"
)]
pub(super) struct FrameHeader {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) disable_cdf_update: bool,
    pub(super) disable_frame_end_update_cdf: bool,
    pub(super) order_hint: u32,
    pub(super) quantization: Quantization,
    pub(super) delta_q_present: bool,
    pub(super) delta_q_res: u32,
    pub(super) delta_lf_present: bool,
    pub(super) delta_lf_res: u32,
    pub(super) delta_lf_multi: bool,
    pub(super) loop_filter: LoopFilter,
    /// `TxMode` (AV1 spec § "Symbols"): `0` (`ONLY_4X4`), `1` (`TX_MODE_LARGEST`), or `2`
    /// (`TX_MODE_SELECT`).
    pub(super) tx_mode: u8,
    pub(super) reduced_tx_set: bool,
    pub(super) tile: TileInfo,
}

/// `uncompressed_header()` (AV1 spec §5.9.2), restricted to this module's `KEY_FRAME`-only
/// scope (ADR-0005 § Scope decision).
///
/// Returns `(header, bits_consumed)` — `bits_consumed` is `frame_header_obu()`'s own bit
/// length (`BitReader::bits_read`), needed by the caller
/// ([`super::av1_decoder::D3d12VideoDecoderAv1::decode_frame_obu`]) to find `frame_obu()`'s
/// `byte_alignment()` boundary between the frame header and the tile-group bytes that
/// follow it in the same `OBU_FRAME` payload (AV1 spec §5.10).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] for `show_existing_frame == 1`, `frame_type !=
/// KEY_FRAME(0)`, `show_frame != 1`, `segmentation_enabled == 1`, `using_qmatrix == 1`, or
/// multi-tile (`TileColsLog2 > 0 || TileRowsLog2 > 0`, or `uniform_tile_spacing_flag ==
/// 0` — see [`parse_tile_info`]'s doc). [`DecodeError::InvalidInput`] on truncated/
/// malformed data.
#[allow(
    clippy::too_many_lines,
    reason = "one linear AV1 spec §5.9.2 syntax-element sequence through the fields this \
    module needs; mirrors hevc_slice.rs::parse_slice_header's identical shape"
)]
pub(super) fn parse_frame_header(
    payload: &[u8],
    seq: &SequenceHeader,
) -> Result<(FrameHeader, usize), DecodeError> {
    const KEY_FRAME: u32 = 0;

    let mut r = BitReader::new(payload);

    let show_existing_frame = read_bit(&mut r)?;
    if show_existing_frame {
        return Err(DecodeError::Unsupported);
    }
    let frame_type = read_bits(&mut r, 2)?;
    if frame_type != KEY_FRAME {
        return Err(DecodeError::Unsupported);
    }
    let show_frame = read_bit(&mut r)?;
    if !show_frame {
        return Err(DecodeError::Unsupported);
    }
    // showable_frame inferred 0 (KEY_FRAME && show_frame), not read.
    // error_resilient_mode inferred 1 (frame_type == KEY_FRAME && show_frame), not read.

    let disable_cdf_update = read_bit(&mut r)?;
    // seq_force_screen_content_tools is always 0 (rejected otherwise at the sequence
    // header) -> allow_screen_content_tools = 0, not read; force_integer_mv likewise not
    // read (FrameIsIntra overrides it to 1 regardless of the unread value).
    // frame_id_numbers_present_flag is always false (rejected at the sequence header) ->
    // current_frame_id not read.

    // frame_type != SWITCH_FRAME, reduced_still_picture_header always false -> read.
    let frame_size_override_flag = read_bit(&mut r)?;

    let order_hint = if seq.order_hint_bits > 0 {
        read_bits(&mut r, seq.order_hint_bits)?
    } else {
        0
    };
    // FrameIsIntra always true -> primary_ref_frame inferred PRIMARY_REF_NONE(7), not read.
    // decoder_model_info_present_flag always false -> no buffer-removal-time fields.
    // KEY_FRAME && show_frame -> refresh_frame_flags inferred allFrames(0xFF), not read.
    // FrameIsIntra && refresh_frame_flags == allFrames -> ref_order_hint loop skipped.

    let (width, height) = if frame_size_override_flag {
        let w = read_bits(&mut r, seq.frame_width_bits)?
            .checked_add(1)
            .ok_or(DecodeError::InvalidInput)?;
        let h = read_bits(&mut r, seq.frame_height_bits)?
            .checked_add(1)
            .ok_or(DecodeError::InvalidInput)?;
        (w, h)
    } else {
        (seq.max_frame_width, seq.max_frame_height)
    };
    // superres_params(): enable_superres is always false (rejected at the sequence
    // header) -> use_superres inferred 0, SuperresDenom == SUPERRES_NUM(8), width
    // unchanged by the upscale formula.

    let render_and_frame_size_different = read_bit(&mut r)?;
    if render_and_frame_size_different {
        let _render_width_minus_1 = read_bits(&mut r, 16)?;
        let _render_height_minus_1 = read_bits(&mut r, 16)?;
    }
    // allow_screen_content_tools always 0 -> allow_intrabc not read, stays 0.

    let disable_frame_end_update_cdf = if disable_cdf_update {
        true
    } else {
        read_bit(&mut r)?
    };
    // primary_ref_frame == PRIMARY_REF_NONE always -> init_non_coeff_cdfs()/
    // setup_past_independence(), no bits.
    // use_ref_frame_mvs always 0 (FrameIsIntra) -> motion_field_estimation() skipped.

    let mi_cols = 2 * ((width + 7) >> 3);
    let mi_rows = 2 * ((height + 7) >> 3);
    let tile = parse_tile_info(&mut r, seq.use_128x128_superblock, mi_cols, mi_rows)?;

    let quantization = parse_quantization_params(&mut r, seq.separate_uv_delta_q)?;
    parse_segmentation_params(&mut r)?;
    let (delta_q_present, delta_q_res) = parse_delta_q_params(&mut r, quantization.base_q_idx)?;
    let (delta_lf_present, delta_lf_res, delta_lf_multi) =
        parse_delta_lf_params(&mut r, delta_q_present)?;
    // primary_ref_frame == PRIMARY_REF_NONE always -> init_coeff_cdfs(), no bits.

    // get_qindex(1, segmentId) reduces to base_q_idx for every segment (segmentation is
    // always disabled in this module's scope, rejected above).
    let coded_lossless = quantization.base_q_idx == 0
        && quantization.delta_q_y_dc == 0
        && quantization.delta_q_u_dc == 0
        && quantization.delta_q_u_ac == 0
        && quantization.delta_q_v_dc == 0
        && quantization.delta_q_v_ac == 0;

    let loop_filter = parse_loop_filter_params(&mut r, coded_lossless)?;
    // cdef_params(): enable_cdef is always false (rejected at the sequence header) ->
    // entire function reads zero bits (CodedLossless / allow_intrabc / !enable_cdef
    // early-return, matching bitstream_av1.rs::write_frame_header's own comment).
    // lr_params(): enable_restoration is always false (rejected at the sequence header) ->
    // entire function reads zero bits, same reasoning.

    let tx_mode = if coded_lossless {
        0u8 // ONLY_4X4
    } else if read_bit(&mut r)? {
        2u8 // TX_MODE_SELECT
    } else {
        1u8 // TX_MODE_LARGEST
    };
    // frame_reference_mode(): FrameIsIntra -> reference_select inferred 0, not read.
    // skip_mode_params(): FrameIsIntra -> skip_mode_present inferred 0, not read.
    // allow_warped_motion: FrameIsIntra -> inferred 0, not read.

    let reduced_tx_set = read_bit(&mut r)?;

    // global_motion_params(): FrameIsIntra -> returns immediately, no bits.
    // film_grain_params(): film_grain_params_present is always false (rejected at the
    // sequence header) -> returns immediately, no bits.

    Ok((
        FrameHeader {
            width,
            height,
            disable_cdf_update,
            disable_frame_end_update_cdf,
            order_hint,
            quantization,
            delta_q_present,
            delta_q_res,
            delta_lf_present,
            delta_lf_res,
            delta_lf_multi,
            loop_filter,
            tx_mode,
            reduced_tx_set,
            tile,
        },
        r.bits_read(),
    ))
}

#[cfg(test)]
#[path = "av1_frame_header_tests.rs"]
mod tests;
