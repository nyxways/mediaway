//! H.264 slice header fields needed to fill `VASliceParameterBufferH264`
//! (ITU-T H.264 § 7.3.3), restricted to this crate's decode scope: **IDR pictures only**,
//! single slice per picture (`first_mb_in_slice == 0`), I slices only, no reference picture
//! reordering / MMCO / weighted prediction (all inapplicable to I slices), no redundant
//! pictures. See `adr/0001-vaapi-h264-cpu-out.md` § Scope.
//!
//! Because there are no reference frames and no MMCO to track, this parser needs no
//! cross-picture decoder state (DPB, `PrevPicOrderCntMsb`, …) — every IDR picture is decoded
//! independently.

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

use super::pps::Pps;
use super::sps::Sps;

/// Parsed slice header fields for one supported (IDR, I-slice, single-slice) picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SliceHeader {
    /// `first_mb_in_slice` — always `0` (single slice per picture is enforced by
    /// [`Self::parse`]).
    pub(super) first_mb_in_slice: u32,
    /// `slice_type % 5` — always `2` (I) (enforced by [`Self::parse`]).
    pub(super) slice_type: u8,
    /// `pic_parameter_set_id` referenced by this slice.
    pub(super) pic_parameter_set_id: u32,
    /// `frame_num`.
    pub(super) frame_num: u32,
    /// `pic_order_cnt_lsb` (valid since `pic_order_cnt_type == 0` is enforced by
    /// [`Sps::parse`]).
    pub(super) pic_order_cnt_lsb: u32,
    /// `slice_qp_delta`.
    pub(super) slice_qp_delta: i32,
    /// `disable_deblocking_filter_idc` (`0` when `deblocking_filter_control_present_flag` is
    /// unset — deblocking enabled by default).
    pub(super) disable_deblocking_filter_idc: u8,
    /// `slice_alpha_c0_offset_div2`.
    pub(super) slice_alpha_c0_offset_div2: i32,
    /// `slice_beta_offset_div2`.
    pub(super) slice_beta_offset_div2: i32,
    /// Bits consumed parsing this header, counted from the start of `rbsp` (i.e. *excluding*
    /// the 1-byte NAL header). Callers building `VASliceParameterBufferH264::slice_data_bit_offset`
    /// must add `8` for the NAL header byte — see ITU-T H.264's VA-API buffer contract comment
    /// ("relative to and includes the NAL unit byte").
    pub(super) bits_consumed: usize,
}

impl SliceHeader {
    /// Parse a slice header from an IDR slice NAL's RBSP (NAL header byte and
    /// emulation-prevention bytes already removed).
    ///
    /// `nal_ref_idc` comes from the NAL header byte (`(header >> 5) & 0b11`); IDR NAL units
    /// always carry `nal_ref_idc != 0` per spec, so `dec_ref_pic_marking()`'s two-flag IDR form
    /// is parsed whenever `nal_ref_idc != 0`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::InvalidInput`] on truncated/malformed data or a
    /// `pic_parameter_set_id`/redundant-picture inconsistency, or
    /// [`DecodeError::Unsupported`] for anything outside this crate's scope: multiple slices
    /// per picture (`first_mb_in_slice != 0`), non-I slice types, or a non-zero
    /// `redundant_pic_cnt`.
    pub(super) fn parse(
        rbsp: &[u8],
        nal_ref_idc: u8,
        sps: &Sps,
        pps: &Pps,
    ) -> Result<Self, DecodeError> {
        let mut r = BitReader::new(rbsp);
        let map_err = |_| DecodeError::InvalidInput;

        let first_mb_in_slice = r.read_ue().map_err(map_err)?;
        if first_mb_in_slice != 0 {
            return Err(DecodeError::Unsupported);
        }
        let slice_type_raw = r.read_ue().map_err(map_err)?;
        let slice_type = u8::try_from(slice_type_raw % 5).unwrap_or(u8::MAX);
        if slice_type != 2 {
            return Err(DecodeError::Unsupported);
        }
        let pic_parameter_set_id = r.read_ue().map_err(map_err)?;
        let frame_num_bits = sps.log2_max_frame_num_minus4 + 4;
        let frame_num = r.read_bits(frame_num_bits).map_err(map_err)?;

        // IDR-only this session: idr_pic_id is always present (dispatch only calls this parser
        // for NalUnitType::IdrSlice).
        let _idr_pic_id = r.read_ue().map_err(map_err)?;

        let poc_lsb_bits = sps.log2_max_pic_order_cnt_lsb_minus4 + 4;
        let pic_order_cnt_lsb = r.read_bits(poc_lsb_bits).map_err(map_err)?;
        if pps.pic_order_present_flag {
            let _delta_pic_order_cnt_bottom = r.read_se().map_err(map_err)?;
        }

        if pps.redundant_pic_cnt_present_flag {
            let redundant_pic_cnt = r.read_ue().map_err(map_err)?;
            if redundant_pic_cnt != 0 {
                return Err(DecodeError::Unsupported);
            }
        }

        if nal_ref_idc != 0 {
            // dec_ref_pic_marking(), IDR form: no_output_of_prior_pics_flag,
            // long_term_reference_flag. Neither affects the parameter buffers this crate
            // builds (no DPB / no long-term reference tracking for IDR-only decode).
            let _no_output_of_prior_pics_flag = r.read_bit().map_err(map_err)?;
            let _long_term_reference_flag = r.read_bit().map_err(map_err)?;
        }

        let slice_qp_delta = r.read_se().map_err(map_err)?;
        let (disable_deblocking_filter_idc, slice_alpha_c0_offset_div2, slice_beta_offset_div2) =
            if pps.deblocking_filter_control_present_flag {
                let idc_raw = r.read_ue().map_err(map_err)?;
                let idc = u8::try_from(idc_raw).map_err(|_| DecodeError::InvalidInput)?;
                if idc == 1 {
                    (idc, 0, 0)
                } else {
                    let alpha = r.read_se().map_err(map_err)?;
                    let beta = r.read_se().map_err(map_err)?;
                    (idc, alpha, beta)
                }
            } else {
                (0u8, 0i32, 0i32)
            };

        let bits_consumed = r.bits_read();

        Ok(Self {
            first_mb_in_slice,
            slice_type,
            pic_parameter_set_id,
            frame_num,
            pic_order_cnt_lsb,
            slice_qp_delta,
            disable_deblocking_filter_idc,
            slice_alpha_c0_offset_div2,
            slice_beta_offset_div2,
            bits_consumed,
        })
    }
}

#[cfg(test)]
#[path = "slice_tests.rs"]
mod tests;
