//! H.264 SPS/PPS parsing (ITU-T H.264 § 7.3.2.1 / § 7.3.2.2), fuller than
//! `mediaway_sw::h264::{Sps, Pps}` — retains `max_num_ref_frames`, `pic_order_cnt_type`
//! 1/2 fields, and every field this module's DXVA-shaped picture-parameter packing
//! ([`super::h264_pic_params`]) needs. See ADR-0002 for why the `mediaway-sw` structs are
//! not reused.
//!
//! Built on the shared, format-agnostic [`mediaway_sw::h264::BitReader`] — only the
//! high-level-syntax parsing here is new.

use mediaway_decoder::DecodeError;
use mediaway_sw::h264::BitReader;
use smallvec::SmallVec;

/// Parsed SPS fields needed for general-GOP H.264 decode (POC, DPB sizing, DXVA
/// picture-parameter packing). Only `chroma_format_idc == 1` (4:2:0) and
/// `frame_mbs_only_flag == true` (no field/MBAFF pictures) are accepted — other values
/// return [`DecodeError::Unsupported`] from [`parse_sps`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Sps {
    pub(super) profile_idc: u8,
    pub(super) level_idc: u8,
    pub(super) seq_parameter_set_id: u32,
    pub(super) bit_depth_luma_minus8: u32,
    pub(super) bit_depth_chroma_minus8: u32,
    /// `log2_max_frame_num_minus4 + 4` — bit width of `frame_num`.
    pub(super) log2_max_frame_num: u32,
    pub(super) pic_order_cnt_type: u32,
    /// `log2_max_pic_order_cnt_lsb_minus4 + 4` (type 0 only; `0` otherwise).
    pub(super) log2_max_pic_order_cnt_lsb: u32,
    pub(super) delta_pic_order_always_zero_flag: bool,
    pub(super) offset_for_non_ref_pic: i32,
    pub(super) offset_for_top_to_bottom_field: i32,
    /// `offset_for_ref_frame[]` (type 1 only) — usually a handful of entries; inline via
    /// `SmallVec` per ADR-0009 (approved for usually-small lists).
    pub(super) offset_for_ref_frame: SmallVec<[i32; 8]>,
    /// `max_num_ref_frames` — drives DPB sizing (see `setup.rs`) and sliding-window
    /// eviction (`h264_refs.rs`).
    pub(super) max_num_ref_frames: u32,
    /// Picture width in macroblocks (coded/DPB-surface width = `mb_width * 16`).
    pub(super) mb_width: u32,
    /// Picture height in macroblocks (coded/DPB-surface height = `mb_height * 16`;
    /// `frame_mbs_only_flag` is required `true`, so this is already the frame height).
    pub(super) mb_height: u32,
    pub(super) direct_8x8_inference_flag: bool,
    /// Cropped (display) width/height in luma samples, after `frame_crop_offset`
    /// (4:2:0 crop units: `CropUnitX = CropUnitY = 2`).
    pub(super) cropped_width: u32,
    pub(super) cropped_height: u32,
}

/// Parsed PPS fields. `num_slice_groups_minus1 != 0` (FMO/ASO) and any explicit
/// weighted-prediction table (`weighted_pred_flag` for P/SP, `weighted_bipred_idc == 1`
/// for B) are rejected by callers before use (see `h264_slice.rs`) — this parser still
/// reads every PPS field so bit position stays correct for whatever follows in the RBSP
/// (there is nothing else in the PPS RBSP, but the same discipline matters for SPS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "mirrors ITU-T H.264's own PPS syntax element list 1:1 — each bool is an \
    independent bitstream flag, not a state machine"
)]
pub(super) struct Pps {
    pub(super) pic_parameter_set_id: u32,
    pub(super) seq_parameter_set_id: u32,
    pub(super) entropy_coding_mode_flag: bool,
    pub(super) bottom_field_pic_order_in_frame_present_flag: bool,
    pub(super) num_slice_groups_minus1: u32,
    pub(super) num_ref_idx_l0_default_active_minus1: u32,
    pub(super) num_ref_idx_l1_default_active_minus1: u32,
    pub(super) weighted_pred_flag: bool,
    pub(super) weighted_bipred_idc: u32,
    pub(super) pic_init_qp_minus26: i32,
    pub(super) pic_init_qs_minus26: i32,
    pub(super) chroma_qp_index_offset: i32,
    pub(super) deblocking_filter_control_present_flag: bool,
    pub(super) constrained_intra_pred_flag: bool,
    pub(super) redundant_pic_cnt_present_flag: bool,
    pub(super) transform_8x8_mode_flag: bool,
    pub(super) second_chroma_qp_index_offset: i32,
}

fn map_bit_err<T>(r: Result<T, mediaway_sw::h264::H264Error>) -> Result<T, DecodeError> {
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

/// Run `scaling_list()`'s Exp-Golomb delta loop (ITU-T H.264 § 7.3.2.1.1.1) to keep
/// bit position correct, discarding the decoded coefficients.
///
/// **Known fidelity gap** (documented, not silent): this module always builds a flat
/// (unscaled, all-16) `DXVA_Qmatrix_H264` regardless of any custom scaling lists a
/// stream signals here — decode still succeeds, but a High-profile stream that relies
/// on custom quantization scaling matrices will not match the source encoder's exact
/// output. See ADR-0002 Addendum.
fn skip_scaling_list(r: &mut BitReader<'_>, size: usize) -> Result<(), DecodeError> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _ in 0..size {
        if next_scale != 0 {
            let delta_scale = read_se(r)?;
            next_scale = (last_scale + delta_scale + 256) % 256;
        }
        last_scale = if next_scale == 0 {
            last_scale
        } else {
            next_scale
        };
    }
    Ok(())
}

const HIGH_CHROMA_PROFILES: [u8; 12] = [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134];

/// Absolute bit index (0 = MSB of the first byte) of the RBSP's `rbsp_stop_one_bit`
/// (ITU-T H.264 § 7.3.2.11, `rbsp_trailing_bits()`) — the last `1` bit in the buffer,
/// since encoders only ever emit `0` padding bits after it to reach a byte boundary.
fn rbsp_stop_bit_pos(rbsp: &[u8]) -> usize {
    for (byte_idx, &byte) in rbsp.iter().enumerate().rev() {
        if byte != 0 {
            let msb_index = 7 - byte.trailing_zeros() as usize;
            return byte_idx * 8 + msb_index;
        }
    }
    0
}

/// `more_rbsp_data()` (ITU-T H.264 § 7.2): whether real syntax elements remain before
/// the trailing `rbsp_stop_one_bit` + zero-alignment padding.
fn more_rbsp_data(bits_read: usize, rbsp: &[u8]) -> bool {
    bits_read < rbsp_stop_bit_pos(rbsp)
}

/// Parse an SPS RBSP (post `NalUnit::parse`, emulation-prevention already removed).
///
/// # Errors
///
/// [`DecodeError::InvalidInput`] on truncated/malformed data or an unsupported
/// `pic_order_cnt_type`. [`DecodeError::Unsupported`] for `chroma_format_idc != 1`,
/// `separate_colour_plane_flag`, or `frame_mbs_only_flag == false` (field/MBAFF).
pub(super) fn parse_sps(rbsp: &[u8]) -> Result<Sps, DecodeError> {
    let mut r = BitReader::new(rbsp);
    let profile_idc = u8::try_from(read_bits(&mut r, 8)?).unwrap_or(0);
    let _constraint_and_reserved = read_bits(&mut r, 8)?;
    let level_idc = u8::try_from(read_bits(&mut r, 8)?).unwrap_or(0);
    let seq_parameter_set_id = read_ue(&mut r)?;

    let mut chroma_format_idc = 1u32;
    let mut separate_colour_plane_flag = false;
    let mut bit_depth_luma_minus8 = 0u32;
    let mut bit_depth_chroma_minus8 = 0u32;
    if HIGH_CHROMA_PROFILES.contains(&profile_idc) {
        chroma_format_idc = read_ue(&mut r)?;
        if chroma_format_idc == 3 {
            separate_colour_plane_flag = read_bit(&mut r)?;
        }
        bit_depth_luma_minus8 = read_ue(&mut r)?;
        bit_depth_chroma_minus8 = read_ue(&mut r)?;
        let _qpprime_y_zero_transform_bypass_flag = read_bit(&mut r)?;
        let seq_scaling_matrix_present_flag = read_bit(&mut r)?;
        if seq_scaling_matrix_present_flag {
            let count = if chroma_format_idc == 3 { 12 } else { 8 };
            for i in 0..count {
                if read_bit(&mut r)? {
                    let size = if i < 6 { 16 } else { 64 };
                    skip_scaling_list(&mut r, size)?;
                }
            }
        }
    }
    if chroma_format_idc != 1 || separate_colour_plane_flag {
        return Err(DecodeError::Unsupported);
    }

    let log2_max_frame_num_minus4 = read_ue(&mut r)?;
    let pic_order_cnt_type = read_ue(&mut r)?;
    let mut log2_max_pic_order_cnt_lsb_minus4 = 0u32;
    let mut delta_pic_order_always_zero_flag = false;
    let mut offset_for_non_ref_pic = 0i32;
    let mut offset_for_top_to_bottom_field = 0i32;
    let mut offset_for_ref_frame: SmallVec<[i32; 8]> = SmallVec::new();
    match pic_order_cnt_type {
        0 => log2_max_pic_order_cnt_lsb_minus4 = read_ue(&mut r)?,
        1 => {
            delta_pic_order_always_zero_flag = read_bit(&mut r)?;
            offset_for_non_ref_pic = read_se(&mut r)?;
            offset_for_top_to_bottom_field = read_se(&mut r)?;
            let num_ref_frames_in_pic_order_cnt_cycle = read_ue(&mut r)?;
            for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
                offset_for_ref_frame.push(read_se(&mut r)?);
            }
        }
        2 => {}
        _ => return Err(DecodeError::InvalidInput),
    }

    let max_num_ref_frames = read_ue(&mut r)?;
    let _gaps_in_frame_num_value_allowed_flag = read_bit(&mut r)?;
    let pic_width_in_mbs_minus1 = read_ue(&mut r)?;
    let pic_height_in_map_units_minus1 = read_ue(&mut r)?;
    let frame_mbs_only_flag = read_bit(&mut r)?;
    if !frame_mbs_only_flag {
        return Err(DecodeError::Unsupported);
    }
    let direct_8x8_inference_flag = read_bit(&mut r)?;
    let frame_cropping_flag = read_bit(&mut r)?;
    let (mut crop_left, mut crop_right, mut crop_top, mut crop_bottom) = (0u32, 0u32, 0u32, 0u32);
    if frame_cropping_flag {
        crop_left = read_ue(&mut r)?;
        crop_right = read_ue(&mut r)?;
        crop_top = read_ue(&mut r)?;
        crop_bottom = read_ue(&mut r)?;
    }
    // vui_parameters() (if present) is not parsed — nothing after it in the RBSP is
    // needed by this module.

    let mb_width = pic_width_in_mbs_minus1 + 1;
    let mb_height = pic_height_in_map_units_minus1 + 1; // frame_mbs_only_flag forced true
    let width = mb_width * 16;
    let height = mb_height * 16;
    let cropped_width = width.saturating_sub(2 * (crop_left + crop_right));
    let cropped_height = height.saturating_sub(2 * (crop_top + crop_bottom));

    Ok(Sps {
        profile_idc,
        level_idc,
        seq_parameter_set_id,
        bit_depth_luma_minus8,
        bit_depth_chroma_minus8,
        log2_max_frame_num: log2_max_frame_num_minus4 + 4,
        pic_order_cnt_type,
        log2_max_pic_order_cnt_lsb: log2_max_pic_order_cnt_lsb_minus4 + 4,
        delta_pic_order_always_zero_flag,
        offset_for_non_ref_pic,
        offset_for_top_to_bottom_field,
        offset_for_ref_frame,
        max_num_ref_frames: max_num_ref_frames.max(1),
        mb_width,
        mb_height,
        direct_8x8_inference_flag,
        cropped_width,
        cropped_height,
    })
}

/// Parse a PPS RBSP (post `NalUnit::parse`).
///
/// # Errors
///
/// [`DecodeError::InvalidInput`] on truncated/malformed data. [`DecodeError::Unsupported`]
/// for `num_slice_groups_minus1 != 0` (FMO/ASO, multi-slice-group).
#[allow(
    clippy::similar_names,
    reason = "pic_init_qp_minus26/pic_init_qs_minus26 intentionally pair ITU-T H.264's own PPS field names"
)]
pub(super) fn parse_pps(rbsp: &[u8]) -> Result<Pps, DecodeError> {
    let mut r = BitReader::new(rbsp);
    let pic_parameter_set_id = read_ue(&mut r)?;
    let seq_parameter_set_id = read_ue(&mut r)?;
    let entropy_coding_mode_flag = read_bit(&mut r)?;
    let bottom_field_pic_order_in_frame_present_flag = read_bit(&mut r)?;
    let num_slice_groups_minus1 = read_ue(&mut r)?;
    if num_slice_groups_minus1 != 0 {
        return Err(DecodeError::Unsupported);
    }
    let num_ref_idx_l0_default_active_minus1 = read_ue(&mut r)?;
    let num_ref_idx_l1_default_active_minus1 = read_ue(&mut r)?;
    let weighted_pred_flag = read_bit(&mut r)?;
    let weighted_bipred_idc = read_bits(&mut r, 2)?;
    let pic_init_qp_minus26 = read_se(&mut r)?;
    let pic_init_qs_minus26 = read_se(&mut r)?;
    let chroma_qp_index_offset = read_se(&mut r)?;
    let deblocking_filter_control_present_flag = read_bit(&mut r)?;
    let constrained_intra_pred_flag = read_bit(&mut r)?;
    let redundant_pic_cnt_present_flag = read_bit(&mut r)?;

    // `more_rbsp_data()`: the trailing High-profile PPS extension (`transform_8x8_mode_flag`
    // onward) is itself optional — absent in Baseline-shaped PPS RBSPs that end right
    // after `redundant_pic_cnt_present_flag`.
    let (transform_8x8_mode_flag, second_chroma_qp_index_offset) =
        if more_rbsp_data(r.bits_read(), rbsp) {
            let transform_8x8_mode_flag = read_bit(&mut r)?;
            let pic_scaling_matrix_present_flag = read_bit(&mut r)?;
            if pic_scaling_matrix_present_flag {
                let count = 6 + if transform_8x8_mode_flag { 2 } else { 0 };
                for i in 0..count {
                    if read_bit(&mut r)? {
                        let size = if i < 6 { 16 } else { 64 };
                        skip_scaling_list(&mut r, size)?;
                    }
                }
            }
            (transform_8x8_mode_flag, read_se(&mut r)?)
        } else {
            (false, chroma_qp_index_offset)
        };

    Ok(Pps {
        pic_parameter_set_id,
        seq_parameter_set_id,
        entropy_coding_mode_flag,
        bottom_field_pic_order_in_frame_present_flag,
        num_slice_groups_minus1,
        num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_default_active_minus1,
        weighted_pred_flag,
        weighted_bipred_idc,
        pic_init_qp_minus26,
        pic_init_qs_minus26,
        chroma_qp_index_offset,
        deblocking_filter_control_present_flag,
        constrained_intra_pred_flag,
        redundant_pic_cnt_present_flag,
        transform_8x8_mode_flag,
        second_chroma_qp_index_offset,
    })
}

#[cfg(test)]
#[path = "h264_sps_pps_tests.rs"]
mod tests;
