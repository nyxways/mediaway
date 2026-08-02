//! HEVC slice-segment-header parsing + short-term reference-picture-set
//! (RPS) construction (ITU-T H.265 § 7.3.6.1, § 7.4.8).
//!
//! HEVC's reference model genuinely differs from H.264's `RefPicList0`/`1`
//! (see `adr/0001`): instead of a sliding-window `frame_num`, each picture
//! signals (or references an SPS-level) short-term RPS describing exactly
//! which prior pictures are "current" references, split into three
//! categories — `RefPicSetStCurrBefore` (POC less than the current
//! picture's), `RefPicSetStCurrAfter` (POC greater), and `RefPicSetLtCurr`
//! (long-term, out of this crate's scope — see below). This file is genuinely
//! new logic, not a rename of `h264_slice.rs`'s ref-list code.
//!
//! **Scope, intentionally partial parse**: this parser reads through short-term
//! RPS construction (the fields this crate's own picture-info/DPB bookkeeping
//! needs) and stops — trailing per-slice syntax (ref-list overrides, weighted
//! prediction, deblocking overrides, entry points) is left unread. The
//! hardware's own bitstream parser receives the complete raw NAL bytes
//! regardless of how far this crate's own sans-io parser reads (same
//! reasoning `h264_slice.rs`'s module doc gives for not building an explicit
//! ref-list struct — the driver parses the real bits itself); this mirrors
//! `mediaway_sw::h264::Sps::parse` stopping before VUI, just applied to a
//! slice header instead of a parameter set. Long-term references, SPS-level
//! RPS lists (`num_short_term_ref_pic_sets > 0`), and `inter_ref_pic_set_prediction_flag`
//! are all rejected upstream by [`crate::hevc_params::HevcSps::parse`] — this
//! module's own RPS parsing only ever sees `short_term_ref_pic_set(0)` with
//! no prediction (the one case SPS-forced-empty-list makes reachable).

#![forbid(unsafe_code)]

use mediaway_sw::h264::{BitReader, H264Error};
use smallvec::SmallVec;

use crate::hevc_params::{HevcNalUnitType, HevcParamError, HevcPps, HevcSps};

/// Decoded, normalized slice type (ITU-T H.265 Table 7-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HevcSliceType {
    /// Bi-predictive slice — not decoded this round (see `decoder.rs`'s HEVC
    /// dispatch scope cut).
    B,
    /// Predictive slice.
    P,
    /// Intra-only slice.
    I,
}

impl HevcSliceType {
    #[must_use]
    const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::B),
            1 => Some(Self::P),
            2 => Some(Self::I),
            _ => None,
        }
    }
}

/// One decoded short-term reference picture entry.
///
/// A signed POC delta relative to the current picture, and whether it is
/// actually used as a reference by the current picture
/// (`used_by_curr_pic_s0/s1_flag`). An entry present in the RPS but not
/// "used by curr" is still tracked in the DPB (as a "foll" picture) but
/// excluded from `RefPicSetStCurrBefore`/`After`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortTermRefPicEntry {
    /// `DeltaPocS0`/`DeltaPocS1` (ITU-T H.265 § 7.4.8) — signed, relative to
    /// the current picture's POC.
    pub delta_poc: i32,
    /// `used_by_curr_pic_s0_flag`/`_s1_flag`.
    pub used_by_curr_pic: bool,
}

/// Parsed `short_term_ref_pic_set(0)` (the only index reachable given this
/// crate's SPS scope cut — see the module doc).
///
/// Holds negative-delta ("before", `S0`) and positive-delta ("after", `S1`)
/// entries, in signal order.
#[derive(Debug, Clone, Default)]
pub struct ShortTermRefPicSet {
    /// `DeltaPocS0`/`UsedByCurrPicS0`, one entry per `num_negative_pics`.
    pub s0: SmallVec<[ShortTermRefPicEntry; 8]>,
    /// `DeltaPocS1`/`UsedByCurrPicS1`, one entry per `num_positive_pics`.
    pub s1: SmallVec<[ShortTermRefPicEntry; 8]>,
}

impl ShortTermRefPicSet {
    /// Parse `short_term_ref_pic_set(stRpsIdx = 0)` (ITU-T H.265 § 7.3.7) —
    /// no `inter_ref_pic_set_prediction_flag` is read (only present when
    /// `stRpsIdx != 0`, unreachable given this crate's scope).
    ///
    /// # Errors
    ///
    /// [`HevcParamError::Bitstream`] on truncated data or overflow, or if the
    /// combined negative+positive picture count exceeds `StdVideoDecodeH265PictureInfo`'s
    /// 8-entry `RefPicSetStCurrBefore`/`RefPicSetStCurrAfter` array capacity.
    pub fn parse(reader: &mut BitReader<'_>) -> Result<Self, HevcParamError> {
        let num_negative_pics = reader.read_ue()?;
        let num_positive_pics = reader.read_ue()?;
        if num_negative_pics > 8 || num_positive_pics > 8 {
            return Err(HevcParamError::Unsupported {
                reason: "short_term_ref_pic_set with more than 8 negative or positive pictures \
                         exceeds StdVideoDecodeH265PictureInfo's RefPicSet array capacity",
            });
        }

        let mut s0 = SmallVec::new();
        let mut delta_poc = 0i32;
        for _ in 0..num_negative_pics {
            let delta_poc_s0_minus1 = reader.read_ue()?;
            let used_by_curr_pic = reader.read_bit()? != 0;
            let step = i32::try_from(delta_poc_s0_minus1)
                .ok()
                .and_then(|v| v.checked_add(1))
                .ok_or(H264Error::FieldOverflow)?;
            delta_poc = delta_poc
                .checked_sub(step)
                .ok_or(H264Error::FieldOverflow)?;
            s0.push(ShortTermRefPicEntry {
                delta_poc,
                used_by_curr_pic,
            });
        }

        let mut s1 = SmallVec::new();
        let mut delta_poc = 0i32;
        for _ in 0..num_positive_pics {
            let delta_poc_s1_minus1 = reader.read_ue()?;
            let used_by_curr_pic = reader.read_bit()? != 0;
            let step = i32::try_from(delta_poc_s1_minus1)
                .ok()
                .and_then(|v| v.checked_add(1))
                .ok_or(H264Error::FieldOverflow)?;
            delta_poc = delta_poc
                .checked_add(step)
                .ok_or(H264Error::FieldOverflow)?;
            s1.push(ShortTermRefPicEntry {
                delta_poc,
                used_by_curr_pic,
            });
        }

        Ok(Self { s0, s1 })
    }

    /// `PocStCurrBefore`/`PocStCurrAfter` (ITU-T H.265 § 8.3.2): the actual
    /// POC values of entries marked `used_by_curr_pic`, in signal order,
    /// relative to `current_poc`.
    #[must_use]
    pub fn curr_before_after_poc(
        &self,
        current_poc: i32,
    ) -> (SmallVec<[i32; 8]>, SmallVec<[i32; 8]>) {
        let before = self
            .s0
            .iter()
            .filter(|entry| entry.used_by_curr_pic)
            .map(|entry| current_poc + entry.delta_poc)
            .collect();
        let after = self
            .s1
            .iter()
            .filter(|entry| entry.used_by_curr_pic)
            .map(|entry| current_poc + entry.delta_poc)
            .collect();
        (before, after)
    }
}

/// Parsed HEVC slice-segment-header fields this crate's decode session
/// needs (see the module doc for the intentional partial-parse boundary).
#[derive(Debug, Clone)]
pub struct HevcSliceSegmentHeader {
    /// Decoded, normalized slice type.
    pub slice_type: HevcSliceType,
    /// `slice_pic_parameter_set_id`.
    pub slice_pic_parameter_set_id: u32,
    /// `slice_pic_order_cnt_lsb` — `None` for IDR pictures (the field is not
    /// present at all; IDR POC is always defined as `0`).
    pub pic_order_cnt_lsb: Option<u32>,
    /// The short-term RPS this slice signaled inline — `None` for IDR
    /// pictures (no RPS at all) or when the picture's total reference count
    /// is otherwise empty.
    pub short_term_rps: Option<ShortTermRefPicSet>,
}

impl HevcSliceSegmentHeader {
    /// Parse a slice-segment-header from `reader`, positioned at the first
    /// bit after the 2-byte NAL header (RBSP already de-emulated, e.g. via
    /// [`crate::hevc_params::HevcNalUnit::parse`]).
    ///
    /// # Errors
    ///
    /// [`HevcParamError::Unsupported`] for B-slices, `first_slice_segment_in_pic_flag
    /// == 0` (multi-slice pictures), or `short_term_ref_pic_set_sps_flag == 1`
    /// (referencing an SPS-level RPS list, which is always empty given this
    /// crate's own `HevcSps::parse` scope cut). Other
    /// [`HevcParamError::Bitstream`] variants on truncated data.
    pub fn parse(
        reader: &mut BitReader<'_>,
        sps: &HevcSps,
        pps: &HevcPps,
        nal_unit_type: HevcNalUnitType,
    ) -> Result<Self, HevcParamError> {
        let first_slice_segment_in_pic_flag = reader.read_bit()? != 0;
        if !first_slice_segment_in_pic_flag {
            return Err(HevcParamError::Unsupported {
                reason: "multi-slice pictures are not supported (first_slice_segment_in_pic_flag must be 1)",
            });
        }
        if matches!(nal_unit_type, HevcNalUnitType::Idr | HevcNalUnitType::Cra) {
            let _no_output_of_prior_pics_flag = reader.read_bit()?;
        }
        let slice_pic_parameter_set_id = reader.read_ue()?;
        // `first_slice_segment_in_pic_flag` is required `true` above, so
        // `dependent_slice_segment_flag`/`slice_segment_address` (only
        // present otherwise) are never read.
        for _ in 0..pps.num_extra_slice_header_bits {
            let _slice_reserved_flag = reader.read_bit()?;
        }
        let slice_type =
            HevcSliceType::from_raw(reader.read_ue()?).ok_or(H264Error::FieldOverflow)?;
        if matches!(slice_type, HevcSliceType::B) {
            return Err(HevcParamError::Unsupported {
                reason: "B-slices are not supported this round",
            });
        }
        if pps.output_flag_present_flag {
            let _pic_output_flag = reader.read_bit()?;
        }
        // `separate_colour_plane_flag` is always `0` in this crate's scope
        // (`chroma_format_idc == 1`, never `3` — see `HevcSps::parse`), so no
        // `colour_plane_id` bits follow.

        let (pic_order_cnt_lsb, short_term_rps) = if nal_unit_type.is_idr() {
            (None, None)
        } else {
            let poc_lsb = reader.read_bits(sps.log2_max_pic_order_cnt_lsb)?;
            let short_term_ref_pic_set_sps_flag = reader.read_bit()? != 0;
            if short_term_ref_pic_set_sps_flag {
                return Err(HevcParamError::Unsupported {
                    reason: "short_term_ref_pic_set_sps_flag == 1 references an SPS-level RPS \
                             list, which this crate's HevcSps::parse always leaves empty",
                });
            }
            let rps = ShortTermRefPicSet::parse(reader)?;
            (Some(poc_lsb), Some(rps))
            // Intentionally stops here — see the module doc's partial-parse
            // boundary (long-term RPS is unreachable per the SPS scope cut;
            // `slice_temporal_mvp_enabled_flag` onward is not read).
        };

        Ok(Self {
            slice_type,
            slice_pic_parameter_set_id,
            pic_order_cnt_lsb,
            short_term_rps,
        })
    }
}

#[cfg(test)]
#[path = "hevc_slice_tests.rs"]
mod tests;
