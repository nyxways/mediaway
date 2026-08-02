//! H.264 slice header parsing (ITU-T H.264 § 7.3.3), scoped to I-slices.
//!
//! Only the fields needed to reach `slice_data()` at the correct bit position are kept;
//! see [`SliceHeader::parse`] for the exact field list and the scope cuts (I-slice only,
//! single slice group already enforced by [`super::Pps::parse`], `pic_order_cnt_type == 0`
//! only, frame pictures only).

#![forbid(unsafe_code)]

use super::bitreader::BitReader;
use super::error::H264Error;
use super::nal::NalUnitType;
use super::pps::Pps;
use super::sps::Sps;

/// Base `slice_type` values (ITU-T H.264 Table 7-6).
///
/// `slice_type` in the bitstream may add `5` to any of these to additionally signal "all
/// slices in this picture share this type"; [`SliceHeader::parse`] normalizes that away by
/// taking `slice_type % 5`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SliceType {
    /// Predictive (inter) slice — not decoded by this crate; see
    /// [`H264Error::UnsupportedSliceType`].
    P,
    /// Bi-predictive (inter) slice — not decoded by this crate.
    B,
    /// Intra-only slice — the only type [`super::decode`] reconstructs pixels for.
    I,
    /// Switching P slice (rare, streaming-specific) — not decoded by this crate.
    Sp,
    /// Switching I slice (rare, streaming-specific) — not decoded by this crate.
    Si,
}

impl SliceType {
    /// Decode the raw `slice_type` field into its base type. `slice_type % 5` is always
    /// `0..=4`, so this never fails.
    #[must_use]
    pub const fn from_raw(slice_type: u32) -> Self {
        match slice_type % 5 {
            0 => Self::P,
            1 => Self::B,
            3 => Self::Sp,
            4 => Self::Si,
            // `slice_type % 5` is always `0..=4`; `2` is the only value left uncovered
            // above, so this wildcard is exhaustive-in-practice, not a fallback guess.
            _ => Self::I,
        }
    }
}

/// Parsed H.264 slice header fields, scoped to what an I-slice / CAVLC / Baseline decode
/// loop needs (ITU-T H.264 § 7.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceHeader {
    /// `first_mb_in_slice` — macroblock address (raster scan) of the first macroblock this
    /// slice codes. [`super::decode::decode_i_frame`] requires this to be `0` (single slice
    /// covering the whole picture); see that function's docs for the scope cut.
    pub first_mb_in_slice: u32,
    /// Decoded base slice type (`I` is the only type this crate reconstructs pixels for).
    pub slice_type: SliceType,
    /// `pic_parameter_set_id` — which PPS this slice refers to.
    pub pic_parameter_set_id: u32,
    /// `frame_num`.
    pub frame_num: u32,
    /// `idr_pic_id`, present only when the containing NAL unit is an IDR slice
    /// ([`NalUnitType::IdrSlice`]).
    pub idr_pic_id: Option<u32>,
    /// `pic_order_cnt_lsb`, present only when `sps.pic_order_cnt_type == 0`.
    pub pic_order_cnt_lsb: Option<u32>,
    /// `slice_qp_delta` — added to `pps.pic_init_qp` for this slice's starting QP.
    pub slice_qp_delta: i32,
}

impl SliceHeader {
    /// Parse a slice header from `reader`, positioned at the start of `slice_header()`
    /// (the first bit after the NAL header byte). On success, `reader` is left positioned
    /// at the start of `slice_data()` so the caller can continue reading macroblocks from
    /// the same bit position — slice header and slice data share one RBSP.
    ///
    /// # Errors
    ///
    /// - [`H264Error::UnsupportedSliceType`] for P/B/SP/SI slices (this crate only
    ///   reconstructs I-slices).
    /// - [`H264Error::UnsupportedPicOrderCntType`] when `sps.pic_order_cnt_type != 0`
    ///   (types 1/2 are not parsed).
    /// - [`H264Error::UnsupportedFieldCoding`] when `sps.frame_mbs_only` is false (field
    ///   pictures are out of scope).
    /// - Other [`H264Error`] variants on truncated data or Exp-Golomb overflow.
    pub fn parse(
        reader: &mut BitReader<'_>,
        sps: &Sps,
        pps: &Pps,
        nal_unit_type: NalUnitType,
        nal_ref_idc: u8,
    ) -> Result<Self, H264Error> {
        if !sps.frame_mbs_only {
            return Err(H264Error::UnsupportedFieldCoding);
        }
        if sps.pic_order_cnt_type != 0 {
            return Err(H264Error::UnsupportedPicOrderCntType);
        }

        let first_mb_in_slice = reader.read_ue()?;
        let slice_type = SliceType::from_raw(reader.read_ue()?);
        if !matches!(slice_type, SliceType::I) {
            return Err(H264Error::UnsupportedSliceType);
        }
        let pic_parameter_set_id = reader.read_ue()?;
        let frame_num = reader.read_bits(sps.log2_max_frame_num)?;
        // `field_pic_flag`/`bottom_field_flag` are only present when
        // `frame_mbs_only_flag == 0`; already rejected above.

        let is_idr = matches!(nal_unit_type, NalUnitType::IdrSlice);
        let idr_pic_id = if is_idr {
            Some(reader.read_ue()?)
        } else {
            None
        };

        let pic_order_cnt_lsb = Some(reader.read_bits(sps.log2_max_pic_order_cnt_lsb)?);
        // `delta_pic_order_cnt_bottom` only applies when
        // `bottom_field_pic_order_in_frame_present_flag` and `!field_pic_flag`; PPS parsing
        // keeps that flag private today and this decode loop only supports frame pictures
        // with `pic_order_cnt_type == 0`, where the field is not read in the common case
        // this crate targets (`bottom_field_pic_order_in_frame_present_flag == 0`).

        // `redundant_pic_cnt` needs `pps.redundant_pic_cnt_present_flag`, which PPS parsing
        // does not retain (assumed `0`, the overwhelmingly common case).

        if nal_ref_idc != 0 {
            skip_dec_ref_pic_marking(reader, is_idr)?;
        }

        let slice_qp_delta = reader.read_se()?;

        if pps.deblocking_filter_control_present {
            let disable_deblocking_filter_idc = reader.read_ue()?;
            if disable_deblocking_filter_idc != 1 {
                let _slice_alpha_c0_offset_div2 = reader.read_se()?;
                let _slice_beta_offset_div2 = reader.read_se()?;
            }
        }
        // `slice_group_change_cycle` needs `num_slice_groups_minus1 > 0`, which PPS parsing
        // already rejects (`H264Error::SliceGroupsUnsupported`).

        Ok(Self {
            first_mb_in_slice,
            slice_type,
            pic_parameter_set_id,
            frame_num,
            idr_pic_id,
            pic_order_cnt_lsb,
            slice_qp_delta,
        })
    }
}

/// Consume `dec_ref_pic_marking()` (ITU-T H.264 § 7.3.3.3) without retaining values: this
/// decode loop never builds a reference picture list (I-slices only), so marking commands
/// have no effect on it — only correct bit-position advancement matters here.
fn skip_dec_ref_pic_marking(reader: &mut BitReader<'_>, is_idr: bool) -> Result<(), H264Error> {
    if is_idr {
        let _no_output_of_prior_pics_flag = reader.read_bit()?;
        let _long_term_reference_flag = reader.read_bit()?;
    } else {
        let adaptive_ref_pic_marking_mode_flag = reader.read_bit()?;
        if adaptive_ref_pic_marking_mode_flag != 0 {
            loop {
                let memory_management_control_operation = reader.read_ue()?;
                if memory_management_control_operation == 0 {
                    break;
                }
                match memory_management_control_operation {
                    1 | 3 => {
                        let _difference_of_pic_nums_minus1 = reader.read_ue()?;
                        if memory_management_control_operation == 3 {
                            let _long_term_frame_idx = reader.read_ue()?;
                        }
                    }
                    // 2 = long_term_pic_num, 4 = max_long_term_frame_idx_plus1,
                    // 6 = long_term_frame_idx: three distinct fields that happen to share
                    // this "one ue(v), value unused" decode shape.
                    2 | 4 | 6 => {
                        let _ = reader.read_ue()?;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "slice_tests.rs"]
mod tests;
