//! `uncompressed_header()` parsing (AV1 spec §5.9.2), `KEY_FRAME`-only branch:
//! `frame_type == KEY_FRAME(0)`, `show_frame == 1` required (a `show_existing_frame`/
//! non-shown keyframe is rejected — no output-reordering/decoder-DPB-replay logic this scope
//! needs). Field presence/order cross-checked (not copied) against
//! `windows::d3d12_video_encode::bitstream_av1::write_frame_header`
//! (`bitstream_av1.rs:204-282`), which already documents, per field, which §5.9.2
//! conditional-inference rule applies for `frame_type == KEY_FRAME`. See
//! [ADR-0003](../../../../adr/linux/0003-vaapi-av1-key-frame-decode.md) § Scope.
//!
//! Unlike the writer's fixed-configuration output, this parser accepts the full range of
//! per-frame values VA-API's decode buffers actually carry (`base_q_idx`, loop-filter levels,
//! `delta_q`/`delta_lf`, `tx_mode`, quantization matrices, screen-content-tools/intra-BC) — it
//! only rejects the sequence-header-gated *optional coding tools* this crate's `KEY_FRAME`
//! scope does not implement (segmentation, CDEF, restoration, superres, film grain — all
//! already rejected by [`super::sequence_header::SequenceHeader::parse`] before this function
//! ever runs, so `cdef_params()`/`lr_params()` are guaranteed to read zero bits here and
//! `segmentation_params()` only needs a single confirming read).

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

use super::bits::su;
use super::sequence_header::SequenceHeader;
use super::tile_info::{self, TileInfo};

/// `seq_force_screen_content_tools` / `seq_force_integer_mv` "read per-frame" sentinel — see
/// [`super::sequence_header`].
const SELECT_VALUE: u32 = 2;

/// `TOTAL_REFS_PER_FRAME` (AV1 spec §3): `loop_filter_ref_deltas` size.
const TOTAL_REFS_PER_FRAME: usize = 8;

/// `setup_past_independence()`'s default `loop_filter_ref_deltas` (AV1 spec §7.20), indexed by
/// reference-frame enum value (`INTRA_FRAME=0, LAST=1, LAST2=2, LAST3=3, GOLDEN=4, BWDREF=5,
/// ALTREF2=6, ALTREF=7`). `setup_past_independence()` always runs for this crate's
/// `KEY_FRAME`-only scope (`primary_ref_frame` is always inferred `PRIMARY_REF_NONE`), so these
/// are the starting values `loop_filter_params()` overwrites from, regardless of whether that
/// function takes its lossless-early-return branch or its full-read branch.
const DEFAULT_REF_DELTAS: [i8; TOTAL_REFS_PER_FRAME] = [1, 0, 0, 0, -1, 0, -1, -1];
/// `setup_past_independence()`'s default `loop_filter_mode_deltas`.
const DEFAULT_MODE_DELTAS: [i8; 2] = [0, 0];

/// `quantization_params()` (AV1 spec §5.9.12) fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QuantizationParams {
    pub(super) base_q_idx: u8,
    pub(super) delta_q_y_dc: i8,
    pub(super) delta_q_u_dc: i8,
    pub(super) delta_q_u_ac: i8,
    pub(super) delta_q_v_dc: i8,
    pub(super) delta_q_v_ac: i8,
    pub(super) using_qmatrix: bool,
    pub(super) qm_y: u8,
    pub(super) qm_u: u8,
    pub(super) qm_v: u8,
}

/// `loop_filter_params()` (AV1 spec §5.9.11) fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LoopFilterParams {
    pub(super) level: [u8; 2],
    pub(super) level_u: u8,
    pub(super) level_v: u8,
    pub(super) sharpness: u8,
    pub(super) delta_enabled: bool,
    pub(super) delta_update: bool,
    pub(super) ref_deltas: [i8; TOTAL_REFS_PER_FRAME],
    pub(super) mode_deltas: [i8; 2],
}

/// Parsed `uncompressed_header()` fields this crate's VA-API decode parameter buffers need, for
/// a supported (`KEY_FRAME`, shown, single-tile) picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool names one AV1 uncompressed_header() syntax element or spec-derived \
              flag; a state machine would obscure the 1:1 spec mapping this crate relies on \
              for review, same precedent as this crate's H.264 Pps"
)]
pub(super) struct FrameHeader {
    pub(super) frame_width_minus1: u16,
    pub(super) frame_height_minus1: u16,
    pub(super) order_hint: u8,
    pub(super) disable_cdf_update: bool,
    pub(super) disable_frame_end_update_cdf: bool,
    pub(super) allow_screen_content_tools: bool,
    pub(super) allow_intrabc: bool,
    pub(super) tile_info: TileInfo,
    pub(super) quantization: QuantizationParams,
    pub(super) delta_q_present: bool,
    pub(super) delta_q_res: u8,
    pub(super) delta_lf_present: bool,
    pub(super) delta_lf_res: u8,
    pub(super) delta_lf_multi: bool,
    pub(super) coded_lossless: bool,
    pub(super) loop_filter: LoopFilterParams,
    /// `TxMode` (AV1 spec §5.9.17): `0` (`ONLY_4X4`, `CodedLossless`), `1` (`TX_MODE_LARGEST`),
    /// or `2` (`TX_MODE_SELECT`).
    pub(super) tx_mode: u32,
    pub(super) reduced_tx_set: bool,
    /// Bits consumed parsing this header, counted from the start of the payload passed to
    /// [`FrameHeader::parse`]. When this header came from an `OBU_FRAME` (header + tile group
    /// sharing one OBU payload), the caller rounds this up to the next byte boundary
    /// (`byte_alignment()`, AV1 spec §5.3.5 — zero-padding, no stop bit, unlike H.264/HEVC's
    /// `rbsp_trailing_bits()`) to find the tile group's start.
    pub(super) bits_consumed: usize,
}

/// `read_delta_q()` (AV1 spec §5.9.13): a coded flag, then `su(1+6)` when set.
fn read_delta_q(r: &mut BitReader<'_>) -> Result<i8, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let delta_coded = r.read_bit().map_err(map_err)? != 0;
    if !delta_coded {
        return Ok(0);
    }
    let value = su(r, 7)?;
    i8::try_from(value).map_err(|_| DecodeError::InvalidInput)
}

#[allow(
    clippy::similar_names,
    reason = "delta_q_{y,u,v}_{dc,ac} are the AV1 spec's own quantization_params() names \
              (§5.9.12) — a 1:1 spec mapping this crate relies on for review, same precedent \
              as this crate's H.264 pic_init_qp_minus26/pic_init_qs_minus26 allow"
)]
fn parse_quantization_params(
    r: &mut BitReader<'_>,
    separate_uv_delta_q: bool,
) -> Result<QuantizationParams, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let base_q_idx = u8::try_from(r.read_bits(8).map_err(map_err)?).unwrap_or(0);
    let delta_q_y_dc = read_delta_q(r)?;
    // NumPlanes > 1 is always true this scope (mono_chrome is rejected by SequenceHeader).
    let diff_uv_delta = separate_uv_delta_q && r.read_bit().map_err(map_err)? != 0;
    let delta_q_u_dc = read_delta_q(r)?;
    let delta_q_u_ac = read_delta_q(r)?;
    let (delta_q_v_dc, delta_q_v_ac) = if diff_uv_delta {
        (read_delta_q(r)?, read_delta_q(r)?)
    } else {
        (delta_q_u_dc, delta_q_u_ac)
    };
    let using_qmatrix = r.read_bit().map_err(map_err)? != 0;
    let (qm_y, qm_u, qm_v) = if using_qmatrix {
        let qm_y = u8::try_from(r.read_bits(4).map_err(map_err)?).unwrap_or(0);
        let qm_u = u8::try_from(r.read_bits(4).map_err(map_err)?).unwrap_or(0);
        let qm_v = if separate_uv_delta_q {
            u8::try_from(r.read_bits(4).map_err(map_err)?).unwrap_or(0)
        } else {
            qm_u
        };
        (qm_y, qm_u, qm_v)
    } else {
        (0, 0, 0)
    };
    Ok(QuantizationParams {
        base_q_idx,
        delta_q_y_dc,
        delta_q_u_dc,
        delta_q_u_ac,
        delta_q_v_dc,
        delta_q_v_ac,
        using_qmatrix,
        qm_y,
        qm_u,
        qm_v,
    })
}

/// `CodedLossless` (AV1 spec §7.12.2 `LosslessArray` derivation), segment-0-only since
/// `segmentation_enabled` is always rejected by [`FrameHeader::parse`] before this is called.
const fn is_coded_lossless(q: &QuantizationParams) -> bool {
    q.base_q_idx == 0
        && q.delta_q_y_dc == 0
        && q.delta_q_u_ac == 0
        && q.delta_q_u_dc == 0
        && q.delta_q_v_ac == 0
        && q.delta_q_v_dc == 0
}

fn parse_loop_filter_params(
    r: &mut BitReader<'_>,
    coded_lossless: bool,
    allow_intrabc: bool,
) -> Result<LoopFilterParams, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    if coded_lossless || allow_intrabc {
        return Ok(LoopFilterParams {
            level: [0, 0],
            level_u: 0,
            level_v: 0,
            sharpness: 0,
            delta_enabled: false,
            delta_update: false,
            ref_deltas: DEFAULT_REF_DELTAS,
            mode_deltas: DEFAULT_MODE_DELTAS,
        });
    }
    let level0 = u8::try_from(r.read_bits(6).map_err(map_err)?).unwrap_or(0);
    let level1 = u8::try_from(r.read_bits(6).map_err(map_err)?).unwrap_or(0);
    // NumPlanes > 1 always true this scope.
    let (level_u, level_v) = if level0 != 0 || level1 != 0 {
        (
            u8::try_from(r.read_bits(6).map_err(map_err)?).unwrap_or(0),
            u8::try_from(r.read_bits(6).map_err(map_err)?).unwrap_or(0),
        )
    } else {
        (0, 0)
    };
    let sharpness = u8::try_from(r.read_bits(3).map_err(map_err)?).unwrap_or(0);
    let delta_enabled = r.read_bit().map_err(map_err)? != 0;
    let mut ref_deltas = DEFAULT_REF_DELTAS;
    let mut mode_deltas = DEFAULT_MODE_DELTAS;
    let delta_update = delta_enabled && r.read_bit().map_err(map_err)? != 0;
    if delta_update {
        for slot in &mut ref_deltas {
            if r.read_bit().map_err(map_err)? != 0 {
                let value = su(r, 7)?;
                *slot = i8::try_from(value).map_err(|_| DecodeError::InvalidInput)?;
            }
        }
        for slot in &mut mode_deltas {
            if r.read_bit().map_err(map_err)? != 0 {
                let value = su(r, 7)?;
                *slot = i8::try_from(value).map_err(|_| DecodeError::InvalidInput)?;
            }
        }
    }
    Ok(LoopFilterParams {
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

impl FrameHeader {
    /// Parse an `OBU_FRAME_HEADER` (or the header prefix of an `OBU_FRAME`) payload — the OBU
    /// header/`leb128` size already stripped by [`super::obu::split_obus`].
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::InvalidInput`] on truncated/malformed data, or
    /// [`DecodeError::Unsupported`] for anything outside this crate's `KEY_FRAME`-only,
    /// single-tile scope (see the module doc).
    #[allow(
        clippy::too_many_lines,
        reason = "one linear, spec-section-ordered read sequence (uncompressed_header() plus \
                  its called sub-syntax-structures); splitting the sub-structures into helper \
                  functions (parse_quantization_params/parse_loop_filter_params, above) already \
                  keeps each individually short — the remainder is uncompressed_header()'s own \
                  single top-level control flow, which has no independently reusable pieces"
    )]
    pub(super) fn parse(data: &[u8], seq: &SequenceHeader) -> Result<Self, DecodeError> {
        let mut r = BitReader::new(data);
        let map_err = |_| DecodeError::InvalidInput;

        let show_existing_frame = r.read_bit().map_err(map_err)? != 0;
        if show_existing_frame {
            return Err(DecodeError::Unsupported);
        }
        let frame_type = r.read_bits(2).map_err(map_err)?;
        if frame_type != 0 {
            return Err(DecodeError::Unsupported); // KEY_FRAME only
        }
        let show_frame = r.read_bit().map_err(map_err)? != 0;
        if !show_frame {
            return Err(DecodeError::Unsupported);
        }
        // frame_type == KEY_FRAME && show_frame -> error_resilient_mode inferred 1 and
        // showable_frame inferred 0, neither read.

        let disable_cdf_update = r.read_bit().map_err(map_err)? != 0;

        let allow_screen_content_tools = if seq.seq_force_screen_content_tools == SELECT_VALUE {
            r.read_bit().map_err(map_err)? != 0
        } else {
            seq.seq_force_screen_content_tools != 0
        };
        // FrameIsIntra unconditionally overrides force_integer_mv to 1 regardless of what is
        // read here, so this crate reads (to keep bit position correct) but discards it.
        if allow_screen_content_tools && seq.seq_force_integer_mv == SELECT_VALUE {
            let _force_integer_mv = r.read_bit().map_err(map_err)?;
        }
        // frame_id_numbers_present_flag == 0 (enforced by SequenceHeader::parse) -> no
        // current_frame_id field.

        let frame_size_override_flag = r.read_bit().map_err(map_err)? != 0;
        let order_hint = r.read_bits(seq.order_hint_bits).map_err(map_err)?;
        let order_hint = u8::try_from(order_hint).unwrap_or(0);
        // FrameIsIntra -> primary_ref_frame inferred PRIMARY_REF_NONE, not read.
        // decoder_model_info_present_flag == 0 (enforced) -> no buffer-removal-time fields.
        // frame_type == KEY_FRAME && show_frame -> refresh_frame_flags inferred allFrames, not
        // read; FrameIsIntra && refresh_frame_flags == allFrames -> ref_order_hint loop skipped.

        let (frame_width_minus1, frame_height_minus1) = if frame_size_override_flag {
            (
                r.read_bits(seq.frame_width_bits_minus_1 + 1)
                    .map_err(map_err)?,
                r.read_bits(seq.frame_height_bits_minus_1 + 1)
                    .map_err(map_err)?,
            )
        } else {
            (seq.max_frame_width_minus_1, seq.max_frame_height_minus_1)
        };
        // superres_params(): enable_superres == 0 (enforced) -> use_superres inferred 0, not
        // read; SuperresDenom == SUPERRES_NUM, UpscaledWidth == FrameWidth, no bits read.

        let render_and_frame_size_different = r.read_bit().map_err(map_err)? != 0;
        if render_and_frame_size_different {
            let _render_width_minus_1 = r.read_bits(16).map_err(map_err)?;
            let _render_height_minus_1 = r.read_bits(16).map_err(map_err)?;
        }
        // UpscaledWidth == FrameWidth (no superres) -> allow_intrabc's own extra size check is
        // always satisfied when allow_screen_content_tools is set.
        let allow_intrabc = allow_screen_content_tools && r.read_bit().map_err(map_err)? != 0;

        let disable_frame_end_update_cdf = if disable_cdf_update {
            true
        } else {
            r.read_bit().map_err(map_err)? != 0
        };
        // primary_ref_frame == PRIMARY_REF_NONE -> init_non_coeff_cdfs()/
        // setup_past_independence(), no bits; use_ref_frame_mvs == 0 -> motion field
        // estimation skipped.

        let frame_width_minus1_u16 =
            u16::try_from(frame_width_minus1).map_err(|_| DecodeError::InvalidInput)?;
        let frame_height_minus1_u16 =
            u16::try_from(frame_height_minus1).map_err(|_| DecodeError::InvalidInput)?;
        let mi_cols = 2 * ((frame_width_minus1 + 1 + 7) >> 3);
        let mi_rows = 2 * ((frame_height_minus1 + 1 + 7) >> 3);
        let tile_info = tile_info::parse(&mut r, seq.use_128x128_superblock, mi_cols, mi_rows)?;

        let quantization = parse_quantization_params(&mut r, seq.separate_uv_delta_q)?;

        // segmentation_params(): parsed far enough to confirm absence (sequence-header-gated
        // optional tool this crate's scope does not implement).
        let segmentation_enabled = r.read_bit().map_err(map_err)? != 0;
        if segmentation_enabled {
            return Err(DecodeError::Unsupported);
        }

        // delta_q_params()
        let delta_q_present = quantization.base_q_idx > 0 && r.read_bit().map_err(map_err)? != 0;
        let delta_q_res = if delta_q_present {
            u8::try_from(r.read_bits(2).map_err(map_err)?).unwrap_or(0)
        } else {
            0
        };

        // delta_lf_params()
        let (delta_lf_present, delta_lf_res, delta_lf_multi) = if delta_q_present {
            let present = !allow_intrabc && r.read_bit().map_err(map_err)? != 0;
            if present {
                (
                    true,
                    u8::try_from(r.read_bits(2).map_err(map_err)?).unwrap_or(0),
                    r.read_bit().map_err(map_err)? != 0,
                )
            } else {
                (false, 0, false)
            }
        } else {
            (false, 0, false)
        };
        // primary_ref_frame == PRIMARY_REF_NONE -> init_coeff_cdfs(), no bits.

        let coded_lossless = is_coded_lossless(&quantization);
        // AllLossless == CodedLossless (no superres this scope -> FrameWidth == UpscaledWidth
        // always).

        let loop_filter = parse_loop_filter_params(&mut r, coded_lossless, allow_intrabc)?;

        // cdef_params(): enable_cdef == 0 (enforced by SequenceHeader::parse) -> reads zero
        // bits. lr_params(): enable_restoration == 0 (enforced) -> reads zero bits.

        // read_tx_mode()
        let tx_mode = if coded_lossless {
            0u32 // ONLY_4X4
        } else if r.read_bit().map_err(map_err)? != 0 {
            2 // TX_MODE_SELECT
        } else {
            1 // TX_MODE_LARGEST
        };

        // frame_reference_mode(): FrameIsIntra -> reference_select inferred 0, not read.
        // skip_mode_params(): FrameIsIntra -> skip_mode_present inferred 0, not read.
        // allow_warped_motion: FrameIsIntra -> inferred 0, not read.

        let reduced_tx_set = r.read_bit().map_err(map_err)? != 0;

        // global_motion_params(): FrameIsIntra -> returns immediately, no bits.
        // film_grain_params(): film_grain_params_present == 0 (enforced) -> returns
        // immediately, no bits.

        Ok(Self {
            frame_width_minus1: frame_width_minus1_u16,
            frame_height_minus1: frame_height_minus1_u16,
            order_hint,
            disable_cdf_update,
            disable_frame_end_update_cdf,
            allow_screen_content_tools,
            allow_intrabc,
            tile_info,
            quantization,
            delta_q_present,
            delta_q_res,
            delta_lf_present,
            delta_lf_res,
            delta_lf_multi,
            coded_lossless,
            loop_filter,
            tx_mode,
            reduced_tx_set,
            bits_consumed: r.bits_read(),
        })
    }
}

#[cfg(test)]
#[path = "frame_header_tests.rs"]
mod tests;
