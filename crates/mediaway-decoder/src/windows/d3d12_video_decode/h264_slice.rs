//! H.264 slice header parsing (ITU-T H.264 § 7.3.3), fuller than
//! `mediaway_sw::h264::SliceHeader` — retains `ref_pic_list_modification`,
//! `dec_ref_pic_marking` (enough to detect and reject adaptive/MMCO marking), and both
//! `pic_order_cnt_type` 1/2 fields. See ADR-0002 for why the `mediaway-sw` struct is not
//! reused, and this crate's `adr/0002-d3d12-native-video-decode.md` § Scope for the
//! sliding-window-only / no-explicit-weighted-prediction cuts this parser enforces.
//!
//! **Scope note (this stage)**: multi-slice pictures are not supported — this module
//! assumes (and [`super::d3d12_video_decode`]'s driving loop enforces) exactly one slice
//! per picture, matching this workspace's other single-slice-only H.264 parsers
//! (`mediaway-sw`, `mediaway-decoder-linux` VA-API).

use crate::DecodeError;
use mediaway_sw::h264::{BitReader, NalUnitType};
use smallvec::SmallVec;

use super::h264_sps_pps::{Pps, Sps};

/// `slice_type` reduced to the five ITU-T-defined values (`slice_type % 5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SliceType {
    P,
    B,
    #[default]
    I,
    Sp,
    Si,
}

impl SliceType {
    const fn from_u32(value: u32) -> Option<Self> {
        match value % 5 {
            0 => Some(Self::P),
            1 => Some(Self::B),
            2 => Some(Self::I),
            3 => Some(Self::Sp),
            4 => Some(Self::Si),
            _ => None,
        }
    }
}

/// One `ref_pic_list_modification()` operation (short-term only — `modification_of_
/// pic_nums_idc == 2`, long-term, is rejected by [`parse_slice_header`] since this stage
/// has no long-term reference support).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RefPicListModOp {
    /// `true` for `modification_of_pic_nums_idc == 1` (add), `false` for `== 0` (subtract).
    pub(super) add: bool,
    pub(super) abs_diff_pic_num_minus1: u32,
}

/// Parsed slice header fields this module needs (POC, reference-list construction,
/// DPB marking decisions). Does not retain `pic_parameter_set_id`/`frame_num` bit widths
/// (callers already hold the active [`Sps`]/[`Pps`] used to parse this header).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct SliceHeader {
    pub(super) slice_type: SliceType,
    pub(super) frame_num: u32,
    pub(super) idr_pic_id: Option<u32>,
    pub(super) pic_order_cnt_lsb: u32,
    pub(super) delta_pic_order_cnt_bottom: i32,
    pub(super) delta_pic_order_cnt: [i32; 2],
    pub(super) direct_spatial_mv_pred_flag: bool,
    pub(super) num_ref_idx_l0_active_minus1: u32,
    pub(super) num_ref_idx_l1_active_minus1: u32,
    pub(super) ref_pic_list_modification_l0: SmallVec<[RefPicListModOp; 4]>,
    pub(super) ref_pic_list_modification_l1: SmallVec<[RefPicListModOp; 4]>,
    /// IDR only (`nal_unit_type == 5`); `false` for non-IDR slices.
    pub(super) no_output_of_prior_pics_flag: bool,
    pub(super) cabac_init_idc: u32,
    pub(super) slice_qp_delta: i32,
    pub(super) disable_deblocking_filter_idc: u32,
    pub(super) slice_alpha_c0_offset_div2: i32,
    pub(super) slice_beta_offset_div2: i32,
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

/// Parse `ref_pic_list_modification()` for one list (ITU-T H.264 § 7.3.3.1).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] on `modification_of_pic_nums_idc == 2` (long-term
/// reference — this stage has no long-term support, see ADR-0002 § Scope).
fn parse_ref_pic_list_modification(
    r: &mut BitReader<'_>,
) -> Result<SmallVec<[RefPicListModOp; 4]>, DecodeError> {
    let mut ops = SmallVec::new();
    if !read_bit(r)? {
        return Ok(ops);
    }
    loop {
        let idc = read_ue(r)?;
        match idc {
            0 | 1 => {
                let abs_diff_pic_num_minus1 = read_ue(r)?;
                ops.push(RefPicListModOp {
                    add: idc == 1,
                    abs_diff_pic_num_minus1,
                });
            }
            2 => return Err(DecodeError::Unsupported),
            3 => break,
            _ => return Err(DecodeError::InvalidInput),
        }
    }
    Ok(ops)
}

/// Parse one slice header for a picture that is the sole slice covering it.
///
/// # Errors
///
/// [`DecodeError::Unsupported`] for SP/SI slices (unparsed `sp_for_switch_flag`/
/// `slice_qs_delta` block — a real, honestly-scoped-out gap, not a silent misparse;
/// real encoders essentially never emit these stream-switching slice types), explicit
/// weighted prediction (`weighted_pred_flag`/`weighted_bipred_idc == 1`), long-term
/// reference marking/modification, or `adaptive_ref_pic_marking_mode_flag` (MMCO) —
/// sliding-window is the only supported reference-marking model this stage (ADR-0002 §
/// Scope). Also returned when a slice is not the first (and only) slice of its picture
/// (`first_mb_in_slice != 0`) or `redundant_pic_cnt != 0`.
#[allow(
    clippy::too_many_lines,
    reason = "one linear slice_header() parse sequence; splitting fragments the bit-position invariant"
)]
/// Returns the parsed header plus the number of bits consumed from `rbsp` — callers
/// (`d3d12_video_decode.rs`) need that bit offset for `DXVA_Slice_H264_Long::
/// BitOffsetToSliceData` (where `slice_data()` begins).
pub(super) fn parse_slice_header(
    rbsp: &[u8],
    nal_unit_type: NalUnitType,
    nal_ref_idc: u8,
    sps: &Sps,
    pps: &Pps,
) -> Result<(SliceHeader, usize), DecodeError> {
    let is_idr = matches!(nal_unit_type, NalUnitType::IdrSlice);
    let mut r = BitReader::new(rbsp);

    let first_mb_in_slice = read_ue(&mut r)?;
    if first_mb_in_slice != 0 {
        return Err(DecodeError::Unsupported);
    }
    let slice_type_raw = read_ue(&mut r)?;
    let slice_type = SliceType::from_u32(slice_type_raw).ok_or(DecodeError::InvalidInput)?;
    if matches!(slice_type, SliceType::Sp | SliceType::Si) {
        // SP/SI slices carry an extra `sp_for_switch_flag`/`slice_qs_delta` block this
        // parser does not read (a genuine gap, not a silent misparse) — real encoders
        // essentially never emit them (stream-switching feature), so this stage
        // rejects them outright rather than risk misaligned bit position.
        return Err(DecodeError::Unsupported);
    }
    let _pic_parameter_set_id = read_ue(&mut r)?;
    let frame_num = read_bits(&mut r, sps.log2_max_frame_num)?;

    // frame_mbs_only_flag is required true by `parse_sps`, so field_pic_flag is never
    // present in the bitstream for any SPS this crate accepted.

    let idr_pic_id = if is_idr { Some(read_ue(&mut r)?) } else { None };

    let mut pic_order_cnt_lsb = 0u32;
    let mut delta_pic_order_cnt_bottom = 0i32;
    let mut delta_pic_order_cnt = [0i32; 2];
    if sps.pic_order_cnt_type == 0 {
        pic_order_cnt_lsb = read_bits(&mut r, sps.log2_max_pic_order_cnt_lsb)?;
        if pps.bottom_field_pic_order_in_frame_present_flag {
            delta_pic_order_cnt_bottom = read_se(&mut r)?;
        }
    } else if sps.pic_order_cnt_type == 1 && !sps.delta_pic_order_always_zero_flag {
        delta_pic_order_cnt[0] = read_se(&mut r)?;
        if pps.bottom_field_pic_order_in_frame_present_flag {
            delta_pic_order_cnt[1] = read_se(&mut r)?;
        }
    }

    if pps.redundant_pic_cnt_present_flag {
        let redundant_pic_cnt = read_ue(&mut r)?;
        if redundant_pic_cnt != 0 {
            return Err(DecodeError::Unsupported);
        }
    }

    let direct_spatial_mv_pred_flag = if matches!(slice_type, SliceType::B) {
        read_bit(&mut r)?
    } else {
        false
    };

    let mut num_ref_idx_l0_active_minus1 = pps.num_ref_idx_l0_default_active_minus1;
    let mut num_ref_idx_l1_active_minus1 = pps.num_ref_idx_l1_default_active_minus1;
    if matches!(slice_type, SliceType::P | SliceType::Sp | SliceType::B) {
        let num_ref_idx_active_override_flag = read_bit(&mut r)?;
        if num_ref_idx_active_override_flag {
            num_ref_idx_l0_active_minus1 = read_ue(&mut r)?;
            if matches!(slice_type, SliceType::B) {
                num_ref_idx_l1_active_minus1 = read_ue(&mut r)?;
            }
        }
    }

    let mut ref_pic_list_modification_l0 = SmallVec::new();
    let mut ref_pic_list_modification_l1 = SmallVec::new();
    if !matches!(slice_type, SliceType::I | SliceType::Si) {
        ref_pic_list_modification_l0 = parse_ref_pic_list_modification(&mut r)?;
        if matches!(slice_type, SliceType::B) {
            ref_pic_list_modification_l1 = parse_ref_pic_list_modification(&mut r)?;
        }
    }

    // Explicit weighted prediction is out of scope this stage (see module doc) — reject
    // before consuming `pred_weight_table()` (its contents are otherwise unused).
    let weighted_table_present = (pps.weighted_pred_flag
        && matches!(slice_type, SliceType::P | SliceType::Sp))
        || (pps.weighted_bipred_idc == 1 && matches!(slice_type, SliceType::B));
    if weighted_table_present {
        return Err(DecodeError::Unsupported);
    }

    let mut no_output_of_prior_pics_flag = false;
    if nal_ref_idc != 0 {
        if is_idr {
            no_output_of_prior_pics_flag = read_bit(&mut r)?;
            let long_term_reference_flag = read_bit(&mut r)?;
            if long_term_reference_flag {
                return Err(DecodeError::Unsupported);
            }
        } else {
            let adaptive_ref_pic_marking_mode_flag = read_bit(&mut r)?;
            if adaptive_ref_pic_marking_mode_flag {
                return Err(DecodeError::Unsupported);
            }
        }
    }

    let cabac_init_idc =
        if pps.entropy_coding_mode_flag && !matches!(slice_type, SliceType::I | SliceType::Si) {
            read_ue(&mut r)?
        } else {
            0u32
        };
    let slice_qp_delta = read_se(&mut r)?;

    let mut disable_deblocking_filter_idc = 0u32;
    let mut slice_alpha_c0_offset_div2 = 0i32;
    let mut slice_beta_offset_div2 = 0i32;
    if pps.deblocking_filter_control_present_flag {
        disable_deblocking_filter_idc = read_ue(&mut r)?;
        if disable_deblocking_filter_idc != 1 {
            slice_alpha_c0_offset_div2 = read_se(&mut r)?;
            slice_beta_offset_div2 = read_se(&mut r)?;
        }
    }
    // `num_slice_groups_minus1 > 0` (slice_group_change_cycle) is unreachable: rejected
    // by `h264_sps_pps::parse_pps` already.

    Ok((
        SliceHeader {
            slice_type,
            frame_num,
            idr_pic_id,
            pic_order_cnt_lsb,
            delta_pic_order_cnt_bottom,
            delta_pic_order_cnt,
            direct_spatial_mv_pred_flag,
            num_ref_idx_l0_active_minus1,
            num_ref_idx_l1_active_minus1,
            ref_pic_list_modification_l0,
            ref_pic_list_modification_l1,
            no_output_of_prior_pics_flag,
            cabac_init_idc,
            slice_qp_delta,
            disable_deblocking_filter_idc,
            slice_alpha_c0_offset_div2,
            slice_beta_offset_div2,
        },
        r.bits_read(),
    ))
}

/// Round a de-emulated-RBSP bit position up to `slice_data()`'s real start for
/// `DXVA_Slice_H264_Long::BitOffsetToSliceData`.
///
/// **Corrected against the official spec** (`DXVA_H264.pdf`, "DirectX Video
/// Acceleration Specification for H.264/AVC Decoding", § `BitOffsetToSliceData`,
/// `docs/standards/registry.toml` id `dxva-h264-decoding`) — a prior session's "Bug 3"
/// fix translated this value into a **raw** NAL bit offset (escape bytes counted back
/// in), reasoning the accelerator needed a raw-buffer position. The spec says the
/// opposite: `BitOffsetToSliceData` **is** the offset within the de-emulated RBSP,
/// relative to `slice_header()`'s first bit — exactly what
/// `parse_slice_header`'s returned bit count already is (slice NAL RBSP starts at
/// `slice_header()`, nothing before it) — and the raw-buffer formula the spec also
/// gives (`BSNALunitDataLocation + (BitOffsetToSliceData >> 3) + 4 + K`) is the
/// **accelerator's own** internal translation, not the host's to perform. The prior
/// translation was silently wrong on any slice header containing an escape byte and is
/// a real candidate for the still-unresolved `DXGI_ERROR_DEVICE_HUNG` hang (ADR-0002
/// Addendum 2026-07-30/08-05).
///
/// One real host-side requirement the spec does impose: for CABAC
/// (`entropy_coding_mode_flag == 1`), the offset must land on the first bit *after*
/// `cabac_alignment_one_bit()` — i.e. byte-aligned (`% 8 == 0`) — not merely the first
/// bit of `slice_data()`.
pub(super) fn bit_offset_to_slice_data(
    deemulated_bits_read: usize,
    entropy_coding_mode_flag: bool,
) -> u32 {
    let bits = if entropy_coding_mode_flag {
        deemulated_bits_read.div_ceil(8) * 8
    } else {
        deemulated_bits_read
    };
    u32::try_from(bits).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "h264_slice_tests.rs"]
mod tests;
