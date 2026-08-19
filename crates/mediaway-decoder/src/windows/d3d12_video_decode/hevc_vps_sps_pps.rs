//! HEVC 2-byte NAL header parsing + SPS/PPS parsing for the D3D12 native decode path.
//!
//! HEVC's NAL header is 2 bytes (`forbidden_zero_bit`, 6-bit `nal_unit_type`, 6-bit
//! `nuh_layer_id`, 3-bit `nuh_temporal_id_plus1`) — **not** `mediaway_sw::h264::NalUnit`'s
//! 1-byte H.264 layout, so this module parses it fresh, same finding
//! `crate::vulkan::hevc_params` already made (ADR-0004 § File layout plan). Built on the
//! shared, codec-agnostic [`mediaway_sw::h264::BitReader`] — only the high-level-syntax
//! parsing here is new/HEVC-specific.
//!
//! Ports the *shape* of `crate::vulkan::hevc_params::{HevcSps, HevcPps}`'s field-by-field
//! parsing (same ITU-T H.265 syntax, sans-io, no Vulkan-specific type touched) into local
//! structs — same "port the shape, not the code" relationship `h264_sps_pps.rs` already
//! has to `mediaway_sw::h264::{Sps, Pps}`.
//!
//! **Real, load-bearing difference from the Vulkan port**: `DXVA_PicParams_HEVC` (see
//! `hevc_pic_params.rs`) has **no profile/tier/level field at all** — unlike the Vulkan
//! decode path (`VkVideoDecodeH265SessionParametersAddInfoKHR` needs a real
//! `StdVideoH265ProfileTierLevel`), this module only needs to consume
//! `profile_tier_level()`'s bits to stay correctly positioned for the SPS fields that
//! follow, not build a `StdVideoH265ProfileTierLevel`/`StdVideoH265LevelIdc` value from it —
//! so there is no `hevc_ptl.rs` submodule here, deliberately (see [`skip_profile_tier_level`]).
//!
//! **VPS is not parsed at all**: nothing in `DXVA_PicParams_HEVC`/`DXVA_Slice_HEVC_Short`/
//! `DXVA_Qmatrix_HEVC` needs any VPS-derived field (unlike the Vulkan path's
//! `StdVideoH265VideoParameterSet`), so a VPS NAL is simply skipped by the top-level NAL
//! dispatch in `hevc_decoder.rs`, same treatment as any other non-SPS/PPS/slice NAL.
//!
//! Scope, mirroring ADR-0004 § Scope decision: Main profile, 8-bit 4:2:0 only (SPS
//! `bit_depth_luma_minus8`/`bit_depth_chroma_minus8` rejected if nonzero), single sub-layer
//! (`sps_max_sub_layers_minus1 == 0`), single tile/no WPP, no PCM, no scaling lists, no
//! SPS-level short-term-RPS list, no long-term references.

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

fn read_ue(r: &mut BitReader<'_>) -> Result<u32, DecodeError> {
    map_bit_err(r.read_ue())
}

fn read_se(r: &mut BitReader<'_>) -> Result<i32, DecodeError> {
    map_bit_err(r.read_se())
}

/// HEVC NAL unit type, reduced to the values this module's decode dispatch acts on
/// (mirrors `crate::vulkan::hevc_params::HevcNalUnitType`'s shape, kept local since that
/// type is crate-internal to the `vulkan` module).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HevcNalUnitType {
    /// Trailing picture, non-reference (`TRAIL_N` 0, `TSA_N` 2, `STSA_N` 4, `RADL_N` 6,
    /// `RASL_N` 8) or reference (`TRAIL_R` 1, `TSA_R` 3, `STSA_R` 5, `RADL_R` 7,
    /// `RASL_R` 9) — the common non-IRAP picture types real encoders emit for this
    /// module's I/P-only scope.
    Trail(u8),
    /// IDR picture, no leading pictures (`IDR_W_RADL` 19 / `IDR_N_LP` 20).
    Idr,
    /// CRA picture (`CRA_NUT`, 21) — **not supported this pass** (see module doc):
    /// unlike the Vulkan HEVC decoder (IDR-only, so CRA vs. IDR never mattered), this
    /// scope's own POC computation (`hevc_poc.rs`) only implements IDR's `PicOrderCntMsb
    /// == 0` reset rule; CRA's own `NoRaslOutputFlag`-gated version of that rule needs
    /// bitstream-position state (first-picture-in-sequence tracking) this module does not
    /// keep. A deliberate, honestly-scoped narrowing beyond even ADR-0004's own named cut.
    Cra,
    Vps,
    Sps,
    Pps,
    /// Any other type value (SEI, AUD, EOS/EOB, RSV/extension types) — safe to skip.
    Other(u8),
}

impl HevcNalUnitType {
    const fn from_u8(value: u8) -> Self {
        match value {
            0..=9 => Self::Trail(value),
            19 | 20 => Self::Idr,
            21 => Self::Cra,
            32 => Self::Vps,
            33 => Self::Sps,
            34 => Self::Pps,
            other => Self::Other(other),
        }
    }

    pub(super) const fn is_idr(self) -> bool {
        matches!(self, Self::Idr)
    }
}

/// Whether a slice NAL of `raw_nal_unit_type` is used as a reference by later pictures
/// (ITU-T H.265 Table 7-1's `_R`/`_N` suffix parity for non-IRAP VCL types; every IRAP
/// type — `16..=23`, which for this module's accepted set means IDR `19`/`20` only, CRA
/// `21` being rejected upstream — is unconditionally a reference picture).
pub(super) const fn is_reference_nal(raw_nal_unit_type: u8) -> bool {
    raw_nal_unit_type > 15 || raw_nal_unit_type % 2 == 1
}

/// One parsed HEVC NAL unit: 2-byte header fields plus RBSP payload (emulation-prevention
/// bytes already removed).
#[derive(Debug, Clone)]
pub(super) struct HevcNalUnit {
    pub(super) unit_type: HevcNalUnitType,
    /// Raw `nal_unit_type` (`0..=63`) — kept alongside the reduced [`HevcNalUnitType`]
    /// because [`is_reference_nal`] needs the exact value (the reduced enum's `Trail(u8)`
    /// already carries it, but `Idr`/`Cra` collapse two distinct raw values each).
    pub(super) raw_nal_unit_type: u8,
    pub(super) rbsp: Vec<u8>,
}

impl HevcNalUnit {
    /// Parse one NAL unit's 2-byte header + de-emulated RBSP from `data` (no start code /
    /// length prefix), e.g. one element of [`mediaway_sw::h264::split_annex_b`].
    ///
    /// # Errors
    ///
    /// [`DecodeError::InvalidInput`] if `data` is shorter than 2 bytes.
    /// [`DecodeError::Unsupported`] if `nuh_layer_id != 0` (multi-layer/scalable HEVC).
    pub(super) fn parse(data: &[u8]) -> Result<Self, DecodeError> {
        let &first = data.first().ok_or(DecodeError::InvalidInput)?;
        let &second = data.get(1).ok_or(DecodeError::InvalidInput)?;
        let raw_nal_unit_type = (first >> 1) & 0x3F;
        let nuh_layer_id = ((first & 0x1) << 5) | (second >> 3);
        if nuh_layer_id != 0 {
            return Err(DecodeError::Unsupported);
        }
        let rbsp = remove_emulation_prevention(data.get(2..).ok_or(DecodeError::InvalidInput)?);
        Ok(Self {
            unit_type: HevcNalUnitType::from_u8(raw_nal_unit_type),
            raw_nal_unit_type,
            rbsp,
        })
    }
}

/// Remove `emulation_prevention_three_byte` (ITU-T H.265 § 7.3.1.1, same `00 00 03` rule
/// as H.264) — duplicated here rather than imported since `mediaway_sw::h264::nal`'s
/// version is private, same reasoning `crate::vulkan::hevc_params` already documents.
fn remove_emulation_prevention(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut zero_run = 0u32;
    for &byte in data {
        if zero_run >= 2 && byte == 0x03 {
            zero_run = 0;
            continue;
        }
        out.push(byte);
        zero_run = if byte == 0 { zero_run + 1 } else { 0 };
    }
    out
}

/// `profile_tier_level(profilePresentFlag=1, maxNumSubLayersMinus1=0)` (ITU-T H.265
/// § 7.3.3) — bits consumed and discarded, not stored (see module doc: `DXVA_PicParams_HEVC`
/// has no profile/tier/level field at all).
fn skip_profile_tier_level(r: &mut BitReader<'_>) -> Result<(), DecodeError> {
    let _general_profile_space = read_bits(r, 2)?;
    let _general_tier_flag = read_bit(r)?;
    let _general_profile_idc = read_bits(r, 5)?;
    let _general_profile_compatibility_flags = read_bits(r, 32)?;
    // general_progressive_source_flag / general_interlaced_source_flag /
    // general_non_packed_constraint_flag / general_frame_only_constraint_flag (4 bits) +
    // 43 general reserved/constraint bits + general_inbld_flag/reserved (1 bit) = 48 bits.
    let _ = read_bits(r, 32)?;
    let _ = read_bits(r, 16)?;
    let _general_level_idc = read_bits(r, 8)?;
    // `sub_layer_profile_present_flag`/`sub_layer_level_present_flag` loop is unreachable:
    // `sps_max_sub_layers_minus1 != 0` is already rejected by [`parse_sps`] before this
    // is called.
    Ok(())
}

/// Parsed HEVC SPS fields this module's DXVA `DXVA_PicParams_HEVC` packing
/// ([`super::hevc_pic_params`]) and DPB/POC bookkeeping need. `Copy` (unlike H.264's
/// `Sps`) — every field is a plain scalar, no owned heap data (HEVC's simpler
/// single-formula POC needs no `offset_for_ref_frame`-style cycle list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is a real, independent ITU-T H.265 SPS flag that must be echoed \
    into DXVA_PicParams_HEVC exactly as signaled — same reasoning h264_sps_pps.rs's Pps \
    gives for its own identical allow"
)]
#[allow(
    clippy::struct_field_names,
    reason = "sps_temporal_mvp_enabled_flag is the real ITU-T H.265 SPS syntax element \
    name (it literally starts with `sps_` in the spec itself) — renaming would obscure \
    the 1:1 spec mapping"
)]
pub(super) struct Sps {
    pub(super) pic_width_in_luma_samples: u32,
    pub(super) pic_height_in_luma_samples: u32,
    /// `log2_max_pic_order_cnt_lsb_minus4 + 4`.
    pub(super) log2_max_pic_order_cnt_lsb: u32,
    /// `sps_max_dec_pic_buffering_minus1[0] + 1` — total DPB storage-buffer count,
    /// **including** the current picture (unlike H.264's `max_num_ref_frames`, which is
    /// references only) — see `hevc_decoder.rs::ensure_session_ready`'s DPB sizing.
    pub(super) max_dec_pic_buffering: u32,
    /// `log2_min_luma_coding_block_size_minus3 + 3`.
    pub(super) log2_min_cb_size: u32,
    pub(super) log2_diff_max_min_cb_size: u32,
    /// `log2_min_luma_transform_block_size_minus2 + 2`.
    pub(super) log2_min_tb_size: u32,
    pub(super) log2_diff_max_min_tb_size: u32,
    pub(super) max_transform_hierarchy_depth_inter: u32,
    pub(super) max_transform_hierarchy_depth_intra: u32,
    pub(super) amp_enabled_flag: bool,
    pub(super) sample_adaptive_offset_enabled_flag: bool,
    pub(super) sps_temporal_mvp_enabled_flag: bool,
    pub(super) strong_intra_smoothing_enabled_flag: bool,
}

/// Parse an SPS RBSP (post [`HevcNalUnit::parse`]).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] for `sps_max_sub_layers_minus1 != 0` (temporal
/// scalability), `chroma_format_idc != 1` / `separate_colour_plane_flag` (non-4:2:0),
/// nonzero `bit_depth_luma_minus8`/`bit_depth_chroma_minus8` (10/12-bit — blocked on the
/// same `mediaway_common::PixelFormat` gap ADR-vulkan-0001 flagged),
/// `scaling_list_enabled_flag == 1`, `pcm_enabled_flag == 1`,
/// `num_short_term_ref_pic_sets > 0` (SPS-level RPS lists), or
/// `long_term_ref_pics_present_flag == 1`. [`DecodeError::InvalidInput`] on
/// truncated/malformed data.
#[allow(
    clippy::too_many_lines,
    reason = "one linear ITU-T H.265 § 7.3.2.2.1 syntax-element sequence through the \
    fields this module needs; splitting would just move consecutive reads of the same \
    RBSP into a same-file helper, mirrors h264_sps_pps.rs::parse_sps's identical shape"
)]
#[allow(
    clippy::similar_names,
    reason = "log2_min_cb_size/log2_min_tb_size and log2_diff_max_min_cb_size/\
    log2_diff_max_min_tb_size are the real ITU-T H.265 § 7.3.2.2.1 syntax element names \
    (coding-block vs transform-block size) — renaming to look less similar would obscure \
    the spec mapping, mirrors crate::vulkan::hevc_params::HevcSps::parse's identical allow"
)]
pub(super) fn parse_sps(rbsp: &[u8]) -> Result<Sps, DecodeError> {
    let mut r = BitReader::new(rbsp);
    let _sps_video_parameter_set_id = read_bits(&mut r, 4)?;
    let sps_max_sub_layers_minus1 = read_bits(&mut r, 3)?;
    if sps_max_sub_layers_minus1 != 0 {
        return Err(DecodeError::Unsupported);
    }
    let _sps_temporal_id_nesting_flag = read_bit(&mut r)?;
    skip_profile_tier_level(&mut r)?;

    let _sps_seq_parameter_set_id = read_ue(&mut r)?;
    let chroma_format_idc = read_ue(&mut r)?;
    if chroma_format_idc == 3 {
        let _separate_colour_plane_flag = read_bit(&mut r)?;
    }
    if chroma_format_idc != 1 {
        return Err(DecodeError::Unsupported);
    }
    let pic_width_in_luma_samples = read_ue(&mut r)?;
    let pic_height_in_luma_samples = read_ue(&mut r)?;
    if read_bit(&mut r)? {
        // conformance_window_flag — cropping is not applied by this module (same
        // documented fidelity gap as h264_sps_pps.rs's own crop handling: the coded
        // picture, not the cropped display picture, is what DXVA/D3D12 addresses here).
        let _conf_win_left_offset = read_ue(&mut r)?;
        let _conf_win_right_offset = read_ue(&mut r)?;
        let _conf_win_top_offset = read_ue(&mut r)?;
        let _conf_win_bottom_offset = read_ue(&mut r)?;
    }
    let bit_depth_luma_minus8 = read_ue(&mut r)?;
    let bit_depth_chroma_minus8 = read_ue(&mut r)?;
    if bit_depth_luma_minus8 != 0 || bit_depth_chroma_minus8 != 0 {
        return Err(DecodeError::Unsupported);
    }
    let log2_max_pic_order_cnt_lsb = read_ue(&mut r)?
        .checked_add(4)
        .ok_or(DecodeError::InvalidInput)?;

    // `sps_max_sub_layers_minus1 == 0` is already enforced above, so regardless of
    // `sps_sub_layer_ordering_info_present_flag`'s value the loop
    // `for (i = start; i <= sps_max_sub_layers_minus1; i++)` always runs exactly once
    // (`start` is either `0` or `sps_max_sub_layers_minus1`, and both are `0` here).
    let _sps_sub_layer_ordering_info_present_flag = read_bit(&mut r)?;
    let max_dec_pic_buffering = read_ue(&mut r)?
        .checked_add(1)
        .ok_or(DecodeError::InvalidInput)?;
    let _sps_max_num_reorder_pics = read_ue(&mut r)?;
    let _sps_max_latency_increase_plus1 = read_ue(&mut r)?;

    let log2_min_cb_size = read_ue(&mut r)?
        .checked_add(3)
        .ok_or(DecodeError::InvalidInput)?;
    let log2_diff_max_min_cb_size = read_ue(&mut r)?;
    let log2_min_tb_size = read_ue(&mut r)?
        .checked_add(2)
        .ok_or(DecodeError::InvalidInput)?;
    let log2_diff_max_min_tb_size = read_ue(&mut r)?;
    let max_transform_hierarchy_depth_inter = read_ue(&mut r)?;
    let max_transform_hierarchy_depth_intra = read_ue(&mut r)?;

    if read_bit(&mut r)? {
        return Err(DecodeError::Unsupported); // scaling_list_enabled_flag
    }
    let amp_enabled_flag = read_bit(&mut r)?;
    let sample_adaptive_offset_enabled_flag = read_bit(&mut r)?;
    if read_bit(&mut r)? {
        return Err(DecodeError::Unsupported); // pcm_enabled_flag
    }

    if read_ue(&mut r)? > 0 {
        return Err(DecodeError::Unsupported); // num_short_term_ref_pic_sets
    }
    if read_bit(&mut r)? {
        return Err(DecodeError::Unsupported); // long_term_ref_pics_present_flag
    }
    let sps_temporal_mvp_enabled_flag = read_bit(&mut r)?;
    let strong_intra_smoothing_enabled_flag = read_bit(&mut r)?;
    // `vui_parameters_present_flag` (+ VUI body) / `sps_extension_present_flag` follow —
    // not parsed, same "parse through what's needed" convention h264_sps_pps.rs documents.

    Ok(Sps {
        pic_width_in_luma_samples,
        pic_height_in_luma_samples,
        log2_max_pic_order_cnt_lsb,
        max_dec_pic_buffering,
        log2_min_cb_size,
        log2_diff_max_min_cb_size,
        log2_min_tb_size,
        log2_diff_max_min_tb_size,
        max_transform_hierarchy_depth_inter,
        max_transform_hierarchy_depth_intra,
        amp_enabled_flag,
        sample_adaptive_offset_enabled_flag,
        sps_temporal_mvp_enabled_flag,
        strong_intra_smoothing_enabled_flag,
    })
}

/// Parsed HEVC PPS fields this module's DXVA packing + `hevc_slice.rs` need. `Copy` for
/// the same reason as [`Sps`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is a real, independent ITU-T H.265 PPS flag that must be echoed \
    into DXVA_PicParams_HEVC exactly as signaled — the driver re-parses the full slice \
    header itself (DXVA_Slice_HEVC_Short carries none of it), so a mismatched flag here \
    desyncs the driver's own parse, not just this module's; same reasoning h264_sps_pps.rs \
    gives for its own identical allow"
)]
#[allow(
    clippy::struct_field_names,
    reason = "pps_cb_qp_offset/pps_cr_qp_offset/pps_slice_chroma_qp_offsets_present_flag/\
    pps_loop_filter_across_slices_enabled_flag are the real ITU-T H.265 PPS syntax \
    element names (they literally start with `pps_` in the spec itself) — renaming \
    would obscure the 1:1 spec mapping"
)]
pub(super) struct Pps {
    pub(super) dependent_slice_segments_enabled_flag: bool,
    pub(super) output_flag_present_flag: bool,
    /// `num_extra_slice_header_bits` — count of `slice_reserved_flag[i]` bits
    /// `hevc_slice.rs` must skip to stay bit-aligned.
    pub(super) num_extra_slice_header_bits: u32,
    pub(super) sign_data_hiding_enabled_flag: bool,
    pub(super) cabac_init_present_flag: bool,
    pub(super) num_ref_idx_l0_default_active_minus1: u32,
    pub(super) num_ref_idx_l1_default_active_minus1: u32,
    pub(super) init_qp_minus26: i32,
    pub(super) constrained_intra_pred_flag: bool,
    pub(super) transform_skip_enabled_flag: bool,
    pub(super) cu_qp_delta_enabled_flag: bool,
    pub(super) diff_cu_qp_delta_depth: u32,
    pub(super) pps_cb_qp_offset: i32,
    pub(super) pps_cr_qp_offset: i32,
    pub(super) pps_slice_chroma_qp_offsets_present_flag: bool,
    pub(super) weighted_pred_flag: bool,
    pub(super) weighted_bipred_flag: bool,
    pub(super) transquant_bypass_enabled_flag: bool,
    pub(super) pps_loop_filter_across_slices_enabled_flag: bool,
    /// `lists_modification_present_flag` — echoed into `DXVA_PicParams_HEVC` for driver
    /// parity, but never actually gates `ref_pic_lists_modification()` presence for any
    /// stream this module accepts: that syntax is only present when
    /// `NumPicTotalCurr > 1`, and `hevc_slice.rs` already rejects every picture whose
    /// combined short-term-RPS `used_by_curr_pic` count isn't exactly `1` (ADR-0004 §
    /// Scope decision), so `NumPicTotalCurr` is always `1` here regardless of this flag.
    pub(super) lists_modification_present_flag: bool,
    pub(super) log2_parallel_merge_level_minus2: u32,
    pub(super) slice_segment_header_extension_present_flag: bool,
}

/// Parse a PPS RBSP (post [`HevcNalUnit::parse`]).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] for `tiles_enabled_flag == 1`,
/// `entropy_coding_sync_enabled_flag == 1` (WPP), `deblocking_filter_control_present_flag
/// == 1`, or `pps_scaling_list_data_present_flag == 1` (only reachable on a
/// non-conformant stream, since SPS `scaling_list_enabled_flag == 1` is already rejected
/// by [`parse_sps`]). [`DecodeError::InvalidInput`] on truncated/malformed data.
#[allow(
    clippy::too_many_lines,
    reason = "one linear ITU-T H.265 § 7.3.2.3.1 syntax-element sequence through the \
    fields DXVA_PicParams_HEVC needs; mirrors h264_sps_pps.rs::parse_pps's identical shape"
)]
pub(super) fn parse_pps(rbsp: &[u8]) -> Result<Pps, DecodeError> {
    let mut r = BitReader::new(rbsp);
    let _pps_pic_parameter_set_id = read_ue(&mut r)?;
    let _pps_seq_parameter_set_id = read_ue(&mut r)?;
    let dependent_slice_segments_enabled_flag = read_bit(&mut r)?;
    let output_flag_present_flag = read_bit(&mut r)?;
    let num_extra_slice_header_bits = read_bits(&mut r, 3)?;
    let sign_data_hiding_enabled_flag = read_bit(&mut r)?;
    let cabac_init_present_flag = read_bit(&mut r)?;
    let num_ref_idx_l0_default_active_minus1 = read_ue(&mut r)?;
    let num_ref_idx_l1_default_active_minus1 = read_ue(&mut r)?;
    let init_qp_minus26 = read_se(&mut r)?;
    let constrained_intra_pred_flag = read_bit(&mut r)?;
    let transform_skip_enabled_flag = read_bit(&mut r)?;
    let cu_qp_delta_enabled_flag = read_bit(&mut r)?;
    let diff_cu_qp_delta_depth = if cu_qp_delta_enabled_flag {
        read_ue(&mut r)?
    } else {
        0
    };
    let pps_cb_qp_offset = read_se(&mut r)?;
    let pps_cr_qp_offset = read_se(&mut r)?;
    let pps_slice_chroma_qp_offsets_present_flag = read_bit(&mut r)?;
    let weighted_pred_flag = read_bit(&mut r)?;
    let weighted_bipred_flag = read_bit(&mut r)?;
    let transquant_bypass_enabled_flag = read_bit(&mut r)?;
    let tiles_enabled_flag = read_bit(&mut r)?;
    let entropy_coding_sync_enabled_flag = read_bit(&mut r)?;
    if tiles_enabled_flag || entropy_coding_sync_enabled_flag {
        return Err(DecodeError::Unsupported);
    }
    let pps_loop_filter_across_slices_enabled_flag = read_bit(&mut r)?;
    let deblocking_filter_control_present_flag = read_bit(&mut r)?;
    if deblocking_filter_control_present_flag {
        return Err(DecodeError::Unsupported);
    }
    if read_bit(&mut r)? {
        return Err(DecodeError::Unsupported); // pps_scaling_list_data_present_flag
    }
    let lists_modification_present_flag = read_bit(&mut r)?;
    let log2_parallel_merge_level_minus2 = read_ue(&mut r)?;
    let slice_segment_header_extension_present_flag = read_bit(&mut r)?;
    // `pps_extension_present_flag` (+ extension body) follows — not parsed, same
    // "parse through what changes bit-parsing for fields this module needs, not
    // everything" convention `h264_sps_pps.rs`/`crate::vulkan::hevc_params` both use.

    Ok(Pps {
        dependent_slice_segments_enabled_flag,
        output_flag_present_flag,
        num_extra_slice_header_bits,
        sign_data_hiding_enabled_flag,
        cabac_init_present_flag,
        num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_default_active_minus1,
        init_qp_minus26,
        constrained_intra_pred_flag,
        transform_skip_enabled_flag,
        cu_qp_delta_enabled_flag,
        diff_cu_qp_delta_depth,
        pps_cb_qp_offset,
        pps_cr_qp_offset,
        pps_slice_chroma_qp_offsets_present_flag,
        weighted_pred_flag,
        weighted_bipred_flag,
        transquant_bypass_enabled_flag,
        pps_loop_filter_across_slices_enabled_flag,
        lists_modification_present_flag,
        log2_parallel_merge_level_minus2,
        slice_segment_header_extension_present_flag,
    })
}

#[cfg(test)]
#[path = "hevc_vps_sps_pps_tests.rs"]
mod tests;
