//! H.264 picture parameter set (PPS) parsing (ITU-T H.264 § 7.3.2.2).

#![forbid(unsafe_code)]

use super::bitreader::BitReader;
use super::error::H264Error;

/// Parsed fields from an H.264 picture parameter set RBSP.
///
/// Only the single-slice-group case (`num_slice_groups_minus1 == 0`) is supported —
/// see [`H264Error::SliceGroupsUnsupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pps {
    /// `pic_parameter_set_id`.
    pub pic_parameter_set_id: u32,
    /// `seq_parameter_set_id` this PPS refers to.
    pub seq_parameter_set_id: u32,
    /// `entropy_coding_mode_flag` (`false` = CAVLC, `true` = CABAC).
    pub entropy_coding_mode: bool,
    /// `num_ref_idx_l0_default_active_minus1 + 1`.
    pub num_ref_idx_l0_default_active: u32,
    /// `num_ref_idx_l1_default_active_minus1 + 1`.
    pub num_ref_idx_l1_default_active: u32,
    /// `pic_init_qp_minus26 + 26`.
    pub pic_init_qp: i32,
    /// `chroma_qp_index_offset` — added to the luma QP before the `QPc` table lookup
    /// (ITU-T H.264 Table 8-15) that derives the chroma quantization parameter.
    pub chroma_qp_index_offset: i32,
    /// `deblocking_filter_control_present_flag`.
    pub deblocking_filter_control_present: bool,
    /// `constrained_intra_pred_flag`.
    pub constrained_intra_pred: bool,
}

impl Pps {
    /// Parse a PPS RBSP (NAL header byte and emulation-prevention bytes already
    /// removed, e.g. the `rbsp` of a [`super::NalUnit`] with
    /// [`super::NalUnitType::Pps`]).
    ///
    /// Parses through `redundant_pic_cnt_present_flag`; PPS extension fields
    /// (`more_rbsp_data()`-gated, e.g. transform-8x8 / scaling-list overrides) are not
    /// read.
    ///
    /// # Errors
    ///
    /// Returns [`H264Error::SliceGroupsUnsupported`] when `num_slice_groups_minus1 > 0`
    /// (multiple slice groups / FMO / ASO), which this parser does not decode. Returns
    /// other [`H264Error`] variants on truncated data or arithmetic overflow while
    /// deriving `+1` / `+26` fields.
    pub fn parse(rbsp: &[u8]) -> Result<Self, H264Error> {
        let mut reader = BitReader::new(rbsp);
        let pic_parameter_set_id = reader.read_ue()?;
        let seq_parameter_set_id = reader.read_ue()?;
        let entropy_coding_mode = reader.read_bit()? != 0;
        let _bottom_field_pic_order_in_frame_present_flag = reader.read_bit()?;
        if reader.read_ue()? > 0 {
            return Err(H264Error::SliceGroupsUnsupported);
        }

        let num_ref_idx_l0_default_active_minus1 = reader.read_ue()?;
        let num_ref_idx_l1_default_active_minus1 = reader.read_ue()?;
        let _weighted_pred_flag = reader.read_bit()?;
        let _weighted_bipred_idc = reader.read_bits(2)?;
        let pic_init_qp_minus26 = reader.read_se()?;
        let _pic_init_qs_minus26 = reader.read_se()?;
        let chroma_qp_index_offset = reader.read_se()?;
        let deblocking_filter_control_present = reader.read_bit()? != 0;
        let constrained_intra_pred = reader.read_bit()? != 0;
        let _redundant_pic_cnt_present_flag = reader.read_bit()?;

        let num_ref_idx_l0_default_active = num_ref_idx_l0_default_active_minus1
            .checked_add(1)
            .ok_or(H264Error::FieldOverflow)?;
        let num_ref_idx_l1_default_active = num_ref_idx_l1_default_active_minus1
            .checked_add(1)
            .ok_or(H264Error::FieldOverflow)?;
        let pic_init_qp = pic_init_qp_minus26
            .checked_add(26)
            .ok_or(H264Error::FieldOverflow)?;

        Ok(Self {
            pic_parameter_set_id,
            seq_parameter_set_id,
            entropy_coding_mode,
            num_ref_idx_l0_default_active,
            num_ref_idx_l1_default_active,
            pic_init_qp,
            chroma_qp_index_offset,
            deblocking_filter_control_present,
            constrained_intra_pred,
        })
    }
}

#[cfg(test)]
#[path = "pps_tests.rs"]
mod tests;
