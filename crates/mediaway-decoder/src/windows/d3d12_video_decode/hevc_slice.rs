//! HEVC slice-segment-header parsing + short-term reference-picture-set (RPS)
//! construction (ITU-T H.265 § 7.3.6.1, § 7.4.8, § 7.3.7).
//!
//! **Ported near-verbatim from `crate::vulkan::hevc_slice::ShortTermRefPicSet`** — pure
//! ITU-T H.265 bitstream logic, zero Vulkan-specific types touched, per ADR-0004 § File
//! layout plan ("the single largest real code-reuse opportunity this ADR has"). Two real
//! additions the Vulkan source has no reason to make (it never wires a P-slice to a real
//! GPU decode call, per ADR-0004 § Correcting the task's premise):
//!
//! 1. This module's own **single-forward-reference scope check**: reject unless the
//!    combined `used_by_curr_pic` count across `short_term_ref_pic_set(0)`'s S0 + S1 sets
//!    is exactly `1` (ADR-0004 § Scope decision).
//! 2. Parses **past** where the Vulkan source intentionally stops (short-term RPS) to
//!    reach `num_ref_idx_l0_active_minus1` — needed to separately enforce
//!    `num_ref_idx_l0_active == 1` (the scope's other named cut) — then stops itself,
//!    since nothing past that point is needed by this module's own DPB/POC/ref-list
//!    bookkeeping. `DXVA_Slice_HEVC_Short` carries no per-slice reference-list/weighted-
//!    prediction/QP detail at all (unlike H.264's `DXVA_Slice_H264_Long`) — the D3D12
//!    accelerator re-parses the **entire** slice-segment header itself from the raw NAL
//!    bytes this module forwards unmodified, so there is no DXVA struct field this
//!    module's own parse needs to reach beyond what its own bookkeeping requires.
//!
//! **Scope note (this stage)**: multi-slice pictures are not supported
//! (`first_slice_segment_in_pic_flag != 1` is rejected), matching this workspace's other
//! single-slice-only HEVC/H.264 parsers.

use smallvec::SmallVec;

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

use super::hevc_vps_sps_pps::{HevcNalUnitType, Pps, Sps};

fn read_bit(r: &mut BitReader<'_>) -> Result<bool, DecodeError> {
    Ok(r.read_bit().map_err(|_err| DecodeError::InvalidInput)? != 0)
}

fn read_bits(r: &mut BitReader<'_>, count: u32) -> Result<u32, DecodeError> {
    r.read_bits(count).map_err(|_err| DecodeError::InvalidInput)
}

fn read_ue(r: &mut BitReader<'_>) -> Result<u32, DecodeError> {
    r.read_ue().map_err(|_err| DecodeError::InvalidInput)
}

/// `slice_type` reduced to the two values this module's scope accepts (B-slices are
/// rejected in [`HevcSliceSegmentHeader::parse`] before this is ever constructed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SliceType {
    P,
    I,
}

impl SliceType {
    const fn from_raw(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::P),
            2 => Some(Self::I),
            _ => None,
        }
    }
}

/// One decoded short-term reference picture entry (`DeltaPocS0`/`S1`,
/// `used_by_curr_pic_s0/s1_flag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShortTermRefPicEntry {
    pub(super) delta_poc: i32,
    pub(super) used_by_curr_pic: bool,
}

/// Parsed `short_term_ref_pic_set(0)` — the only index reachable given this module's own
/// SPS scope cut (SPS-level RPS lists are rejected by `hevc_vps_sps_pps::parse_sps`, so
/// every slice signals its RPS inline with `stRpsIdx == num_short_term_ref_pic_sets == 0`,
/// meaning no `inter_ref_pic_set_prediction_flag` is ever read — that syntax element is
/// only present when `stRpsIdx != 0`).
#[derive(Debug, Clone, Default)]
pub(super) struct ShortTermRefPicSet {
    pub(super) s0: SmallVec<[ShortTermRefPicEntry; 8]>,
    pub(super) s1: SmallVec<[ShortTermRefPicEntry; 8]>,
}

impl ShortTermRefPicSet {
    /// Parse `short_term_ref_pic_set(stRpsIdx = 0)` (ITU-T H.265 § 7.3.7).
    ///
    /// # Errors
    ///
    /// [`DecodeError::Unsupported`] if the combined negative+positive picture count
    /// exceeds `DXVA_PicParams_HEVC`'s 8-entry `RefPicSetStCurrBefore`/`After` array
    /// capacity. [`DecodeError::InvalidInput`] on truncated/overflowing data.
    fn parse(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let num_negative_pics = read_ue(r)?;
        let num_positive_pics = read_ue(r)?;
        if num_negative_pics > 8 || num_positive_pics > 8 {
            return Err(DecodeError::Unsupported);
        }

        let mut s0 = SmallVec::new();
        let mut delta_poc = 0i32;
        for _ in 0..num_negative_pics {
            let delta_poc_s0_minus1 = read_ue(r)?;
            let used_by_curr_pic = read_bit(r)?;
            let step = i32::try_from(delta_poc_s0_minus1)
                .ok()
                .and_then(|v| v.checked_add(1))
                .ok_or(DecodeError::InvalidInput)?;
            delta_poc = delta_poc
                .checked_sub(step)
                .ok_or(DecodeError::InvalidInput)?;
            s0.push(ShortTermRefPicEntry {
                delta_poc,
                used_by_curr_pic,
            });
        }

        let mut s1 = SmallVec::new();
        let mut delta_poc = 0i32;
        for _ in 0..num_positive_pics {
            let delta_poc_s1_minus1 = read_ue(r)?;
            let used_by_curr_pic = read_bit(r)?;
            let step = i32::try_from(delta_poc_s1_minus1)
                .ok()
                .and_then(|v| v.checked_add(1))
                .ok_or(DecodeError::InvalidInput)?;
            delta_poc = delta_poc
                .checked_add(step)
                .ok_or(DecodeError::InvalidInput)?;
            s1.push(ShortTermRefPicEntry {
                delta_poc,
                used_by_curr_pic,
            });
        }

        Ok(Self { s0, s1 })
    }

    /// Combined `used_by_curr_pic` count across S0 + S1 (`NumPicTotalCurr`, since this
    /// module's scope has no long-term references) — must be exactly `1` for a P-slice
    /// (ADR-0004 § Scope decision).
    pub(super) fn num_curr_pics(&self) -> usize {
        self.s0.iter().filter(|e| e.used_by_curr_pic).count()
            + self.s1.iter().filter(|e| e.used_by_curr_pic).count()
    }

    /// `PocStCurrBefore`/`PocStCurrAfter` (ITU-T H.265 § 8.3.2): actual POC values of
    /// entries marked `used_by_curr_pic`, relative to `current_poc`.
    pub(super) fn curr_before_after_poc(
        &self,
        current_poc: i32,
    ) -> (SmallVec<[i32; 8]>, SmallVec<[i32; 8]>) {
        let before = self
            .s0
            .iter()
            .filter(|e| e.used_by_curr_pic)
            .map(|e| current_poc + e.delta_poc)
            .collect();
        let after = self
            .s1
            .iter()
            .filter(|e| e.used_by_curr_pic)
            .map(|e| current_poc + e.delta_poc)
            .collect();
        (before, after)
    }

    /// Every POC value this RPS names, regardless of `used_by_curr_pic` — the DPB
    /// "RPS application" (bumping) eviction rule (ITU-T H.265 § 8.3.2) evicts any current
    /// reference **not** in this set, not just the (smaller) "used by current picture"
    /// subset — a `used_by_curr_pic == false` entry is still a "foll" picture the RPS
    /// keeps alive for a *future* picture's own reference use.
    pub(super) fn all_poc(&self, current_poc: i32) -> SmallVec<[i32; 16]> {
        self.s0
            .iter()
            .chain(self.s1.iter())
            .map(|e| current_poc + e.delta_poc)
            .collect()
    }
}

/// Parsed HEVC slice-segment-header fields this module's DPB/POC/ref-list bookkeeping
/// needs (see module doc for the intentional partial-parse boundary).
#[derive(Debug, Clone)]
pub(super) struct SliceHeader {
    pub(super) slice_type: SliceType,
    /// `None` for IDR pictures (the field is not present; IDR POC is always `0`).
    pub(super) pic_order_cnt_lsb: Option<u32>,
    /// `None` for IDR pictures (no RPS at all).
    pub(super) short_term_rps: Option<ShortTermRefPicSet>,
    /// `num_ref_idx_l0_active_minus1` — only meaningful for P-slices; `0` for I-slices
    /// (which have no reference list at all).
    pub(super) num_ref_idx_l0_active_minus1: u32,
    /// Bits consumed by `short_term_ref_pic_set(0)` itself (excluding
    /// `slice_pic_order_cnt_lsb`/`short_term_ref_pic_set_sps_flag`) — `DXVA_PicParams_HEVC
    /// ::wNumBitsForShortTermRPSInSlice`, `0` when `short_term_rps` is `None` (IDR).
    pub(super) short_term_rps_bits: u32,
}

/// Parse a slice-segment-header (post [`super::hevc_vps_sps_pps::HevcNalUnit::parse`]).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] for B-slices, `first_slice_segment_in_pic_flag == 0`
/// (multi-slice pictures), `short_term_ref_pic_set_sps_flag == 1` (SPS-level RPS list,
/// always empty given this module's own SPS scope cut), a combined RPS
/// `used_by_curr_pic` count other than exactly `1` on a P-slice, or
/// `num_ref_idx_l0_active != 1` (whether via `num_ref_idx_active_override_flag` or the
/// PPS default) — both the scope's named single-forward-reference cuts (ADR-0004 §
/// Scope decision). [`DecodeError::InvalidInput`] on truncated/malformed data.
#[allow(
    clippy::too_many_lines,
    reason = "one linear slice_segment_header() parse sequence; splitting fragments the \
    bit-position invariant, mirrors h264_slice.rs::parse_slice_header's identical shape"
)]
pub(super) fn parse_slice_header(
    rbsp: &[u8],
    nal_unit_type: HevcNalUnitType,
    sps: &Sps,
    pps: &Pps,
) -> Result<SliceHeader, DecodeError> {
    let mut r = BitReader::new(rbsp);

    let first_slice_segment_in_pic_flag = read_bit(&mut r)?;
    if !first_slice_segment_in_pic_flag {
        return Err(DecodeError::Unsupported);
    }
    if matches!(nal_unit_type, HevcNalUnitType::Idr | HevcNalUnitType::Cra) {
        let _no_output_of_prior_pics_flag = read_bit(&mut r)?;
    }
    let _slice_pic_parameter_set_id = read_ue(&mut r)?;
    // `first_slice_segment_in_pic_flag` is required `true` above, so
    // `dependent_slice_segment_flag`/`slice_segment_address` (only present otherwise) are
    // never read.
    for _ in 0..pps.num_extra_slice_header_bits {
        let _slice_reserved_flag = read_bit(&mut r)?;
    }
    let slice_type_raw = read_ue(&mut r)?;
    let slice_type = SliceType::from_raw(slice_type_raw).ok_or(DecodeError::Unsupported)?;
    if pps.output_flag_present_flag {
        let _pic_output_flag = read_bit(&mut r)?;
    }
    // `separate_colour_plane_flag` is always `0` in this module's scope
    // (`chroma_format_idc == 1`, never `3` — see `hevc_vps_sps_pps::parse_sps`), so no
    // `colour_plane_id` bits follow.

    let is_idr = nal_unit_type.is_idr();
    let (pic_order_cnt_lsb, short_term_rps, short_term_rps_bits) = if is_idr {
        (None, None, 0)
    } else {
        let poc_lsb = read_bits(&mut r, sps.log2_max_pic_order_cnt_lsb)?;
        let short_term_ref_pic_set_sps_flag = read_bit(&mut r)?;
        if short_term_ref_pic_set_sps_flag {
            return Err(DecodeError::Unsupported);
        }
        let rps_bits_start = r.bits_read();
        let rps = ShortTermRefPicSet::parse(&mut r)?;
        let rps_bits = u32::try_from(r.bits_read() - rps_bits_start).unwrap_or(u32::MAX);
        if matches!(slice_type, SliceType::P) && rps.num_curr_pics() != 1 {
            return Err(DecodeError::Unsupported);
        }
        if sps.sps_temporal_mvp_enabled_flag {
            let _slice_temporal_mvp_enabled_flag = read_bit(&mut r)?;
        }
        (Some(poc_lsb), Some(rps), rps_bits)
    };

    if sps.sample_adaptive_offset_enabled_flag {
        let _slice_sao_luma_flag = read_bit(&mut r)?;
        // `ChromaArrayType != 0` is always true in this module's scope
        // (`chroma_format_idc == 1`, `separate_colour_plane_flag == 0`).
        let _slice_sao_chroma_flag = read_bit(&mut r)?;
    }

    let num_ref_idx_l0_active_minus1 = if matches!(slice_type, SliceType::P) {
        let num_ref_idx_active_override_flag = read_bit(&mut r)?;
        let value = if num_ref_idx_active_override_flag {
            read_ue(&mut r)?
        } else {
            pps.num_ref_idx_l0_default_active_minus1
        };
        if value != 0 {
            return Err(DecodeError::Unsupported);
        }
        // `ref_pic_lists_modification()` is only present when
        // `lists_modification_present_flag && NumPicTotalCurr > 1` — `NumPicTotalCurr`
        // is always `1` here (the `rps.num_curr_pics() != 1` check above already
        // enforced it), so that syntax is guaranteed absent regardless of
        // `pps.lists_modification_present_flag`'s value; nothing more to read.
        value
    } else {
        0
    };
    // Intentionally stops here — see module doc's partial-parse boundary. Everything
    // past this point (`pred_weight_table()`, `five_minus_max_num_merge_cand`,
    // `slice_qp_delta`, deblocking overrides, entry points, ...) is re-parsed by the
    // D3D12 accelerator itself from the raw NAL bytes this module forwards unmodified.

    Ok(SliceHeader {
        slice_type,
        pic_order_cnt_lsb,
        short_term_rps,
        num_ref_idx_l0_active_minus1,
        short_term_rps_bits,
    })
}

#[cfg(test)]
#[path = "hevc_slice_tests.rs"]
mod tests;
