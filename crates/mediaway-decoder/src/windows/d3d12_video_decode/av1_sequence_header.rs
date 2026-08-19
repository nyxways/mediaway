//! `sequence_header_obu()` + `color_config()` parsing (AV1 spec §5.5.1/§5.5.2) into a
//! local [`SequenceHeader`].
//!
//! Field-by-field cross-checked against `mediaway-encoder-windows`'s
//! `d3d12_video_encode/bitstream_av1.rs::write_sequence_header`'s own exhaustive
//! inference-rule comments — that function is a writer, but every "not read because X"
//! comment there names exactly which reader-side branch this module must take for the
//! same all-fixed shape (ADR-0005 § File layout plan).
//!
//! **Real, deliberate scope narrowing beyond ADR-0005's literal rejection list**: this
//! module also rejects `timing_info_present_flag == 1`, `initial_display_delay_present_flag
//! == 1`, and `operating_points_cnt_minus_1 != 0` — none of the three is in the ADR's own
//! written-out reject list, but all three gate deep, rarely-exercised spec surface
//! (`timing_info()`/`decoder_model_info()`/`operating_parameters_info()`, multi-operating-
//! point scalability signaling) that no field of `DXVA_PicParams_AV1` ever consumes and
//! that this crate's own AV1 encoder (the realistic same-workspace test source, ADR-0005
//! § Context) never sets. Narrowing here keeps parsing tractable and correct rather than
//! silently mis-parsing a real but exotic stream — mirrors HEVC's own CRA rejection, which
//! ADR-0004's own wiki page describes as going "beyond even ADR-0004's own named cut."
//!
//! Uses the shared, codec-agnostic [`mediaway_sw::h264::BitReader`] for AV1's own `f(n)`
//! fixed-width reads — same precedent `hevc_vps_sps_pps.rs` already set (ADR-0005 § Reuse).
//! **Not reused**: `read_ue`/`read_se` (H.264 Exp-Golomb, structurally unrelated to AV1's
//! own `uvlc()`/`leb128()`/`su(n)` variable-length codes) — this module needs none of them
//! (every sequence-header field is `f(n)`).

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::{BitReader, H264Error};

fn map_bit_err<T>(r: Result<T, H264Error>) -> Result<T, DecodeError> {
    r.map_err(|_err| DecodeError::InvalidInput)
}

fn read_bit(r: &mut BitReader<'_>) -> Result<bool, DecodeError> {
    Ok(map_bit_err(r.read_bit())? != 0)
}

fn read_bits(r: &mut BitReader<'_>, count: u32) -> Result<u32, DecodeError> {
    map_bit_err(r.read_bits(count))
}

/// AV1 Main profile (`seq_profile == 0`) — 4:2:0 chroma, 8/10-bit; this module's scope
/// further restricts to 8-bit only (ADR-0005 § Scope decision).
const SEQ_PROFILE_MAIN: u32 = 0;
/// `SELECT_SCREEN_CONTENT_TOOLS` / `SELECT_INTEGER_MV` (AV1 spec § 3, "Symbols and
/// abbreviated terms").
const SELECT_SCREEN_CONTENT_TOOLS: u32 = 2;

/// Parsed AV1 sequence-header fields this module's frame-header parsing and DXVA packing
/// ([`super::av1_pic_params`]) need. Every field this module's scope allows to vary is a
/// real bitstream-derived value, not a hardcoded constant — only the fields this module
/// rejects nonzero (`enable_cdef`/`enable_restoration`/`enable_superres`/
/// `film_grain_params_present`/screen-content-tools/`reduced_still_picture_header`) are
/// guaranteed-zero by construction once [`parse_sequence_header`] returns `Ok`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is a real, independent AV1 sequence-header flag that must be \
    echoed into DXVA_PicParams_AV1 exactly as signaled — same reasoning \
    hevc_vps_sps_pps.rs's Sps gives for its own identical allow"
)]
pub(super) struct SequenceHeader {
    pub(super) max_frame_width: u32,
    pub(super) max_frame_height: u32,
    /// `frame_width_bits_minus_1 + 1` — bit width of `frame_size()`'s
    /// `frame_width_minus_1` when `frame_size_override_flag == 1`.
    pub(super) frame_width_bits: u32,
    pub(super) frame_height_bits: u32,
    pub(super) use_128x128_superblock: bool,
    pub(super) enable_filter_intra: bool,
    pub(super) enable_intra_edge_filter: bool,
    pub(super) enable_interintra_compound: bool,
    pub(super) enable_masked_compound: bool,
    pub(super) enable_dual_filter: bool,
    pub(super) enable_order_hint: bool,
    pub(super) enable_jnt_comp: bool,
    pub(super) enable_ref_frame_mvs: bool,
    /// `order_hint_bits_minus_1 + 1`, or `0` when `enable_order_hint == 0` (AV1 spec
    /// §5.5.1's own `OrderHintBits` derivation).
    pub(super) order_hint_bits: u32,
    pub(super) separate_uv_delta_q: bool,
}

/// Parse a sequence-header OBU payload (post [`super::av1_obu::split_obus`]).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] for `seq_profile != 0`, `reduced_still_picture_header ==
/// 1`, `timing_info_present_flag == 1`, `initial_display_delay_present_flag == 1`,
/// `operating_points_cnt_minus_1 != 0` (both narrowings beyond ADR-0005's literal list, see
/// module doc), `frame_id_numbers_present_flag == 1`, non-zero `seq_force_screen_content_
/// tools` (screen-content-tools/palette rejected outright), `enable_superres == 1`,
/// `enable_cdef == 1`, `enable_restoration == 1`, `high_bitdepth == 1`, `mono_chrome == 1`,
/// non-4:2:0 subsampling, or `film_grain_params_present == 1`. [`DecodeError::InvalidInput`]
/// on truncated/malformed data.
#[allow(
    clippy::too_many_lines,
    reason = "one linear AV1 spec §5.5.1/§5.5.2 syntax-element sequence through the fields \
    this module needs; mirrors hevc_vps_sps_pps.rs::parse_sps's identical shape"
)]
pub(super) fn parse_sequence_header(payload: &[u8]) -> Result<SequenceHeader, DecodeError> {
    let mut r = BitReader::new(payload);

    let seq_profile = read_bits(&mut r, 3)?;
    if seq_profile != SEQ_PROFILE_MAIN {
        return Err(DecodeError::Unsupported);
    }
    let _still_picture = read_bit(&mut r)?;
    let reduced_still_picture_header = read_bit(&mut r)?;
    if reduced_still_picture_header {
        return Err(DecodeError::Unsupported);
    }

    let timing_info_present_flag = read_bit(&mut r)?;
    if timing_info_present_flag {
        // Would require parsing timing_info()/decoder_model_info() to stay positioned —
        // rejected outright, see module doc.
        return Err(DecodeError::Unsupported);
    }
    let initial_display_delay_present_flag = read_bit(&mut r)?;
    if initial_display_delay_present_flag {
        return Err(DecodeError::Unsupported);
    }
    let operating_points_cnt_minus_1 = read_bits(&mut r, 5)?;
    if operating_points_cnt_minus_1 != 0 {
        return Err(DecodeError::Unsupported);
    }
    let _operating_point_idc0 = read_bits(&mut r, 12)?;
    let seq_level_idx0 = read_bits(&mut r, 5)?;
    if seq_level_idx0 > 7 {
        let _seq_tier0 = read_bit(&mut r)?;
    }
    // decoder_model_info_present_flag == 0 (forced by timing_info_present_flag == 0) ->
    // decoder_model_present_for_this_op[0] not read.
    // initial_display_delay_present_flag == 0 -> initial_display_delay_present_for_this_op[0]
    // not read.

    let frame_width_bits = read_bits(&mut r, 4)?
        .checked_add(1)
        .ok_or(DecodeError::InvalidInput)?;
    let frame_height_bits = read_bits(&mut r, 4)?
        .checked_add(1)
        .ok_or(DecodeError::InvalidInput)?;
    let max_frame_width = read_bits(&mut r, frame_width_bits)?
        .checked_add(1)
        .ok_or(DecodeError::InvalidInput)?;
    let max_frame_height = read_bits(&mut r, frame_height_bits)?
        .checked_add(1)
        .ok_or(DecodeError::InvalidInput)?;

    // reduced_still_picture_header == 0 (rejected above) -> frame_id_numbers_present_flag
    // is always read (not inferred 0).
    let frame_id_numbers_present_flag = read_bit(&mut r)?;
    if frame_id_numbers_present_flag {
        // Would require delta_frame_id_length_minus_2/additional_frame_id_length_minus_1
        // to stay positioned, plus current_frame_id parsing in every frame header —
        // rejected outright, see module doc.
        return Err(DecodeError::Unsupported);
    }

    let use_128x128_superblock = read_bit(&mut r)?;
    let enable_filter_intra = read_bit(&mut r)?;
    let enable_intra_edge_filter = read_bit(&mut r)?;

    // reduced_still_picture_header == 0 (rejected above) -> the else branch always taken.
    let enable_interintra_compound = read_bit(&mut r)?;
    let enable_masked_compound = read_bit(&mut r)?;
    let _enable_warped_motion = read_bit(&mut r)?;
    let enable_dual_filter = read_bit(&mut r)?;
    let enable_order_hint = read_bit(&mut r)?;
    let (enable_jnt_comp, enable_ref_frame_mvs) = if enable_order_hint {
        (read_bit(&mut r)?, read_bit(&mut r)?)
    } else {
        (false, false)
    };
    let seq_choose_screen_content_tools = read_bit(&mut r)?;
    let seq_force_screen_content_tools = if seq_choose_screen_content_tools {
        SELECT_SCREEN_CONTENT_TOOLS
    } else {
        read_bits(&mut r, 1)?
    };
    if seq_force_screen_content_tools > 0 {
        // Screen-content-tools/palette use is out of scope (ADR-0005 § Scope decision) —
        // rejected before reading seq_choose_integer_mv/seq_force_integer_mv, which are
        // otherwise irrelevant to this module (never consumed by DXVA_PicParams_AV1).
        return Err(DecodeError::Unsupported);
    }
    let order_hint_bits = if enable_order_hint {
        read_bits(&mut r, 3)?
            .checked_add(1)
            .ok_or(DecodeError::InvalidInput)?
    } else {
        0
    };

    let enable_superres = read_bit(&mut r)?;
    if enable_superres {
        return Err(DecodeError::Unsupported);
    }
    let enable_cdef = read_bit(&mut r)?;
    if enable_cdef {
        return Err(DecodeError::Unsupported);
    }
    let enable_restoration = read_bit(&mut r)?;
    if enable_restoration {
        return Err(DecodeError::Unsupported);
    }

    let separate_uv_delta_q = parse_color_config(&mut r)?;

    let film_grain_params_present = read_bit(&mut r)?;
    if film_grain_params_present {
        return Err(DecodeError::Unsupported);
    }

    Ok(SequenceHeader {
        max_frame_width,
        max_frame_height,
        frame_width_bits,
        frame_height_bits,
        use_128x128_superblock,
        enable_filter_intra,
        enable_intra_edge_filter,
        enable_interintra_compound,
        enable_masked_compound,
        enable_dual_filter,
        enable_order_hint,
        enable_jnt_comp,
        enable_ref_frame_mvs,
        order_hint_bits,
        separate_uv_delta_q,
    })
}

/// `color_config()` (AV1 spec §5.5.2), restricted to this module's `seq_profile == 0`
/// scope. Returns `separate_uv_delta_q` (the only field this module's frame-header parsing
/// still needs afterward).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] for `high_bitdepth == 1`, `mono_chrome == 1`, or any
/// resulting `(subsampling_x, subsampling_y) != (1, 1)` (non-4:2:0 — reachable even at
/// `seq_profile == 0` via the `color_primaries == BT_709 && ... == IDENTITY` branch, AV1
/// spec §5.5.2, which forces 4:4:4 regardless of profile).
fn parse_color_config(r: &mut BitReader<'_>) -> Result<bool, DecodeError> {
    const CP_BT_709: u32 = 1;
    const TC_SRGB: u32 = 13;
    const MC_IDENTITY: u32 = 0;

    let high_bitdepth = read_bit(r)?;
    if high_bitdepth {
        return Err(DecodeError::Unsupported);
    }
    // seq_profile == 0 (enforced by the caller) -> mono_chrome is always read (not
    // inferred 0, unlike seq_profile == 1).
    let mono_chrome = read_bit(r)?;
    if mono_chrome {
        return Err(DecodeError::Unsupported);
    }
    let color_description_present_flag = read_bit(r)?;
    let (color_primaries, transfer_characteristics, matrix_coefficients) =
        if color_description_present_flag {
            (read_bits(r, 8)?, read_bits(r, 8)?, read_bits(r, 8)?)
        } else {
            // CP_UNSPECIFIED / TC_UNSPECIFIED / MC_UNSPECIFIED (AV1 spec § 6.4.2 Table 5).
            (2, 2, 2)
        };

    // mono_chrome == 0 (rejected above) -> this branch, not the mono_chrome == 1 branch.
    let (subsampling_x, subsampling_y) = if color_primaries == CP_BT_709
        && transfer_characteristics == TC_SRGB
        && matrix_coefficients == MC_IDENTITY
    {
        // color_range is spec-inferred 1 here, not read.
        (0u32, 0u32)
    } else {
        let _color_range = read_bit(r)?;
        // seq_profile == 0 (enforced by the caller) -> subsampling_x = subsampling_y = 1,
        // not read.
        (1u32, 1u32)
    };
    if subsampling_x != 1 || subsampling_y != 1 {
        return Err(DecodeError::Unsupported);
    }
    if subsampling_x == 1 && subsampling_y == 1 {
        let _chroma_sample_position = read_bits(r, 2)?;
    }
    let separate_uv_delta_q = read_bit(r)?;
    Ok(separate_uv_delta_q)
}

#[cfg(test)]
#[path = "av1_sequence_header_tests.rs"]
mod tests;
