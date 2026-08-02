//! H.264 SPS/PPS parsing for decode, plus `StdVideoDecodeH264*`/`StdVideoH264*`
//! construction.
//!
//! Reuses [`mediaway_sw::h264::BitReader`] for bit-level RBSP reading and
//! [`mediaway_sw::h264::{NalUnit, NalUnitType, split_annex_b}`] for Annex-B
//! framing + NAL header parsing (both codec-generic, not IDR-only-shaped).
//! Does **not** reuse `mediaway_sw::h264::{Sps, Pps}` — per `adr/0001`'s
//! "Bitstream-parser reuse" section, those are shaped for the sibling
//! all-intra CAVLC pixel decoder (no `max_num_ref_frames`, no
//! `weighted_pred_flag`, no P-slice-shaped parsing) and are not extended
//! here to avoid entangling this crate with that decoder's own scope.
//!
//! Scope, matching `adr/0001` and the D3D12 sibling ADR's cuts: 4:2:0 8-bit
//! only (`chroma_format_idc == 1`), progressive only
//! (`frame_mbs_only_flag == 1`), `pic_order_cnt_type == 0` only, single
//! slice group (`num_slice_groups_minus1 == 0`).

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "every count here comes from an Exp-Golomb-decoded H.264 syntax element, always \
              small in practice (SPS/PPS ids, ref-frame counts, QP deltas) — narrowing casts \
              into the small Std*/vulkanalia field widths mirror mediaway-encoder-vulkan's own \
              h264_params.rs allow for the identical shape (encode direction)."
)]

use mediaway_sw::h264::{BitReader, H264Error};
use thiserror::Error;
use vulkanalia::vk::video as native;

/// Errors from parsing an H.264 SPS/PPS, or a syntax element this crate's
/// scope cuts do not support.
///
/// Crate-internal — wrapped into [`crate::session::VulkanDecodeError`] at the
/// boundary that talks to Vulkan, and ultimately mapped to
/// `mediaway_decoder::DecodeError` (see `decoder.rs`).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum H264ParamError {
    /// Truncated data or an Exp-Golomb/arithmetic overflow while reading.
    #[error(transparent)]
    Bitstream(#[from] H264Error),
    /// A syntax element this crate's scope does not decode (see the module
    /// doc's scope list).
    #[error("unsupported H.264 syntax: {reason}")]
    Unsupported {
        /// Human-readable reason, always a `'static` literal at call sites.
        reason: &'static str,
    },
}

/// Parsed H.264 sequence parameter set fields this crate's decode session needs.
///
/// A superset of `mediaway_sw::h264::Sps` (adds `max_num_ref_frames`, keeps
/// `pic_order_cnt_type` itself rather than discarding non-zero values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct H264Sps {
    /// `seq_parameter_set_id`.
    pub seq_parameter_set_id: u32,
    /// `profile_idc`.
    pub profile_idc: u8,
    /// `level_idc`.
    pub level_idc: u8,
    /// `log2_max_frame_num_minus4 + 4`.
    pub log2_max_frame_num: u32,
    /// `1 << log2_max_frame_num` — `MaxFrameNum` (ITU-T H.264 § 7.4.2.1.1).
    pub max_frame_num: u32,
    /// `log2_max_pic_order_cnt_lsb_minus4 + 4`.
    pub log2_max_pic_order_cnt_lsb: u32,
    /// `max_num_ref_frames` — sizes this session's DPB (see `dpb.rs`).
    pub max_num_ref_frames: u32,
    /// `pic_width_in_mbs_minus1 + 1`.
    pub pic_width_in_mbs: u32,
    /// `pic_height_in_map_units_minus1 + 1` (equal to picture height in
    /// macroblocks: `frame_mbs_only_flag` is required `true` — see
    /// [`H264Sps::parse`]'s scope cut).
    pub pic_height_in_map_units: u32,
    /// Cropped picture width in luma samples.
    pub width: u32,
    /// Cropped picture height in luma samples.
    pub height: u32,
}

impl H264Sps {
    /// Parse an SPS RBSP (NAL header byte already stripped, e.g. via
    /// [`mediaway_sw::h264::NalUnit::parse`]).
    ///
    /// # Errors
    ///
    /// [`H264ParamError::Unsupported`] when `chroma_format_idc != 1`,
    /// `frame_mbs_only_flag == 0`, or `pic_order_cnt_type != 0`. Other
    /// [`H264ParamError::Bitstream`] variants on truncated/overflowing data.
    #[allow(
        clippy::too_many_lines,
        reason = "linear ITU-T H.264 § 7.3.2.1.1 syntax-element sequence — splitting further \
                  would just move consecutive reads of the same RBSP into a same-file helper"
    )]
    pub fn parse(rbsp: &[u8]) -> Result<Self, H264ParamError> {
        let profile_idc = *rbsp.first().ok_or(H264Error::UnexpectedEof)?;
        let level_idc = *rbsp.get(2).ok_or(H264Error::UnexpectedEof)?;
        let mut reader = BitReader::new(rbsp.get(3..).ok_or(H264Error::UnexpectedEof)?);

        let seq_parameter_set_id = reader.read_ue()?;

        let mut chroma_format_idc = 1u32;
        if profile_has_chroma_fields(profile_idc) {
            chroma_format_idc = reader.read_ue()?;
            if chroma_format_idc == 3 {
                let _separate_colour_plane_flag = reader.read_bit()?;
            }
            let _bit_depth_luma_minus8 = reader.read_ue()?;
            let _bit_depth_chroma_minus8 = reader.read_ue()?;
            let _qpprime_y_zero_transform_bypass_flag = reader.read_bit()?;
            if reader.read_bit()? != 0 {
                let scaling_list_count = if chroma_format_idc == 3 { 12 } else { 8 };
                skip_scaling_lists(&mut reader, scaling_list_count)?;
            }
        }
        if chroma_format_idc != 1 {
            return Err(H264ParamError::Unsupported {
                reason: "only chroma_format_idc == 1 (4:2:0) is supported",
            });
        }

        let log2_max_frame_num = reader
            .read_ue()?
            .checked_add(4)
            .ok_or(H264Error::FieldOverflow)?;
        let max_frame_num = 1u32
            .checked_shl(log2_max_frame_num)
            .ok_or(H264Error::FieldOverflow)?;

        let pic_order_cnt_type = reader.read_ue()?;
        let mut log2_max_pic_order_cnt_lsb = 0u32;
        match pic_order_cnt_type {
            0 => {
                log2_max_pic_order_cnt_lsb = reader
                    .read_ue()?
                    .checked_add(4)
                    .ok_or(H264Error::FieldOverflow)?;
            }
            1 => {
                let _delta_pic_order_always_zero_flag = reader.read_bit()?;
                let _offset_for_non_ref_pic = reader.read_se()?;
                let _offset_for_top_to_bottom_field = reader.read_se()?;
                let cycle = reader.read_ue()?;
                for _ in 0..cycle {
                    let _offset_for_ref_frame = reader.read_se()?;
                }
            }
            _ => {}
        }
        if pic_order_cnt_type != 0 {
            return Err(H264ParamError::Unsupported {
                reason: "only pic_order_cnt_type == 0 is supported",
            });
        }

        let max_num_ref_frames = reader.read_ue()?;
        let _gaps_in_frame_num_value_allowed_flag = reader.read_bit()?;

        let pic_width_in_mbs = reader
            .read_ue()?
            .checked_add(1)
            .ok_or(H264Error::FieldOverflow)?;
        let pic_height_in_map_units = reader
            .read_ue()?
            .checked_add(1)
            .ok_or(H264Error::FieldOverflow)?;
        let frame_mbs_only_flag = reader.read_bit()? != 0;
        if !frame_mbs_only_flag {
            return Err(H264ParamError::Unsupported {
                reason: "only frame_mbs_only_flag == 1 (progressive) is supported",
            });
        }
        let _direct_8x8_inference_flag = reader.read_bit()?;

        let (crop_left, crop_right, crop_top, crop_bottom) = if reader.read_bit()? != 0 {
            (
                reader.read_ue()?,
                reader.read_ue()?,
                reader.read_ue()?,
                reader.read_ue()?,
            )
        } else {
            (0, 0, 0, 0)
        };

        // 4:2:0 + frame-only, so crop units are fixed: crop_unit_x = SubWidthC = 2,
        // crop_unit_y = SubHeightC * frame_height_factor = 2 * 1 = 2
        // (ITU-T H.264 § 7.4.2.1.1, Table 6-1).
        let raw_width = pic_width_in_mbs
            .checked_mul(16)
            .ok_or(H264Error::FieldOverflow)?;
        let raw_height = pic_height_in_map_units
            .checked_mul(16)
            .ok_or(H264Error::FieldOverflow)?;
        let crop_width = crop_left
            .checked_add(crop_right)
            .and_then(|sum| sum.checked_mul(2))
            .ok_or(H264Error::FieldOverflow)?;
        let crop_height = crop_top
            .checked_add(crop_bottom)
            .and_then(|sum| sum.checked_mul(2))
            .ok_or(H264Error::FieldOverflow)?;
        let width = raw_width
            .checked_sub(crop_width)
            .ok_or(H264Error::FieldOverflow)?;
        let height = raw_height
            .checked_sub(crop_height)
            .ok_or(H264Error::FieldOverflow)?;

        Ok(Self {
            seq_parameter_set_id,
            profile_idc,
            level_idc,
            log2_max_frame_num,
            max_frame_num,
            log2_max_pic_order_cnt_lsb,
            max_num_ref_frames,
            pic_width_in_mbs,
            pic_height_in_map_units,
            width,
            height,
        })
    }

    /// Build the `StdVideoH264SequenceParameterSet` this SPS's fields
    /// describe, for `VkVideoDecodeH264SessionParametersAddInfoKHR`.
    #[must_use]
    pub fn to_std(&self) -> native::StdVideoH264SequenceParameterSet {
        let mut flags = native::StdVideoH264SpsFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 2]),
            __bindgen_padding_0: 0,
        };
        flags.set_frame_mbs_only_flag(1);
        flags.set_direct_8x8_inference_flag(1);
        native::StdVideoH264SequenceParameterSet {
            flags,
            profile_idc: native::StdVideoH264ProfileIdc(i32::from(self.profile_idc)),
            level_idc: native::StdVideoH264LevelIdc(i32::from(self.level_idc)),
            chroma_format_idc: native::STD_VIDEO_H264_CHROMA_FORMAT_IDC_420,
            seq_parameter_set_id: self.seq_parameter_set_id as u8,
            bit_depth_luma_minus8: 0,
            bit_depth_chroma_minus8: 0,
            log2_max_frame_num_minus4: (self.log2_max_frame_num - 4) as u8,
            pic_order_cnt_type: native::STD_VIDEO_H264_POC_TYPE_0,
            offset_for_non_ref_pic: 0,
            offset_for_top_to_bottom_field: 0,
            log2_max_pic_order_cnt_lsb_minus4: (self.log2_max_pic_order_cnt_lsb - 4) as u8,
            num_ref_frames_in_pic_order_cnt_cycle: 0,
            max_num_ref_frames: self.max_num_ref_frames as u8,
            reserved1: 0,
            pic_width_in_mbs_minus1: self.pic_width_in_mbs - 1,
            pic_height_in_map_units_minus1: self.pic_height_in_map_units - 1,
            frame_crop_left_offset: 0,
            frame_crop_right_offset: 0,
            frame_crop_top_offset: 0,
            frame_crop_bottom_offset: 0,
            reserved2: 0,
            pOffsetForRefFrame: core::ptr::null(),
            pScalingLists: core::ptr::null(),
            pSequenceParameterSetVui: core::ptr::null(),
        }
    }
}

/// Parsed H.264 picture parameter set fields this crate's decode session needs.
///
/// Not reused from `mediaway_sw::h264::Pps` per the module doc — this version
/// additionally keeps `weighted_pred_flag`/`weighted_bipred_idc` (needed for
/// `StdVideoH264PictureParameterSet`, discarded by the sibling all-intra
/// decoder's `Pps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct H264Pps {
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
    /// `weighted_pred_flag`.
    pub weighted_pred_flag: bool,
    /// `weighted_bipred_idc` (`0..=2`).
    pub weighted_bipred_idc: u32,
    /// `pic_init_qp_minus26 + 26`.
    pub pic_init_qp: i32,
    /// `chroma_qp_index_offset`.
    pub chroma_qp_index_offset: i32,
    /// `deblocking_filter_control_present_flag`.
    pub deblocking_filter_control_present: bool,
}

impl H264Pps {
    /// Parse a PPS RBSP (NAL header byte already stripped).
    ///
    /// # Errors
    ///
    /// [`H264ParamError::Unsupported`] when `num_slice_groups_minus1 > 0`
    /// (multiple slice groups / FMO / ASO) or `redundant_pic_cnt_present_flag
    /// == 1` (this crate's slice-header parser does not read
    /// `redundant_pic_cnt`, so a stream signaling it would silently misparse
    /// every slice header downstream — rejected here instead, at the PPS
    /// boundary, rather than failing confusingly deep in slice parsing).
    /// Other [`H264ParamError::Bitstream`] variants on truncated/overflowing
    /// data.
    pub fn parse(rbsp: &[u8]) -> Result<Self, H264ParamError> {
        let mut reader = BitReader::new(rbsp);
        let pic_parameter_set_id = reader.read_ue()?;
        let seq_parameter_set_id = reader.read_ue()?;
        let entropy_coding_mode = reader.read_bit()? != 0;
        let _bottom_field_pic_order_in_frame_present_flag = reader.read_bit()?;
        if reader.read_ue()? > 0 {
            return Err(H264ParamError::Unsupported {
                reason: "num_slice_groups_minus1 > 0 (multiple slice groups) is not supported",
            });
        }

        let num_ref_idx_l0_default_active = reader
            .read_ue()?
            .checked_add(1)
            .ok_or(H264Error::FieldOverflow)?;
        let num_ref_idx_l1_default_active = reader
            .read_ue()?
            .checked_add(1)
            .ok_or(H264Error::FieldOverflow)?;
        let weighted_pred_flag = reader.read_bit()? != 0;
        let weighted_bipred_idc = reader.read_bits(2)?;
        let pic_init_qp_minus26 = reader.read_se()?;
        let _pic_init_qs_minus26 = reader.read_se()?;
        let chroma_qp_index_offset = reader.read_se()?;
        let deblocking_filter_control_present = reader.read_bit()? != 0;
        let _constrained_intra_pred_flag = reader.read_bit()?;
        let redundant_pic_cnt_present_flag = reader.read_bit()? != 0;
        if redundant_pic_cnt_present_flag {
            return Err(H264ParamError::Unsupported {
                reason: "redundant_pic_cnt_present_flag is not supported",
            });
        }

        let pic_init_qp = pic_init_qp_minus26
            .checked_add(26)
            .ok_or(H264Error::FieldOverflow)?;

        Ok(Self {
            pic_parameter_set_id,
            seq_parameter_set_id,
            entropy_coding_mode,
            num_ref_idx_l0_default_active,
            num_ref_idx_l1_default_active,
            weighted_pred_flag,
            weighted_bipred_idc,
            pic_init_qp,
            chroma_qp_index_offset,
            deblocking_filter_control_present,
        })
    }

    /// Build the `StdVideoH264PictureParameterSet` this PPS's fields
    /// describe, for `VkVideoDecodeH264SessionParametersAddInfoKHR`.
    #[must_use]
    pub fn to_std(&self) -> native::StdVideoH264PictureParameterSet {
        let mut flags = native::StdVideoH264PpsFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
            __bindgen_padding_0: [0; 3],
        };
        flags.set_entropy_coding_mode_flag(u32::from(self.entropy_coding_mode));
        flags.set_weighted_pred_flag(u32::from(self.weighted_pred_flag));
        flags.set_deblocking_filter_control_present_flag(u32::from(
            self.deblocking_filter_control_present,
        ));
        native::StdVideoH264PictureParameterSet {
            flags,
            seq_parameter_set_id: self.seq_parameter_set_id as u8,
            pic_parameter_set_id: self.pic_parameter_set_id as u8,
            num_ref_idx_l0_default_active_minus1: (self.num_ref_idx_l0_default_active - 1) as u8,
            num_ref_idx_l1_default_active_minus1: (self.num_ref_idx_l1_default_active - 1) as u8,
            weighted_bipred_idc: native::StdVideoH264WeightedBipredIdc(
                self.weighted_bipred_idc as i32,
            ),
            pic_init_qp_minus26: (self.pic_init_qp - 26) as i8,
            pic_init_qs_minus26: 0,
            chroma_qp_index_offset: self.chroma_qp_index_offset as i8,
            second_chroma_qp_index_offset: self.chroma_qp_index_offset as i8,
            pScalingLists: core::ptr::null(),
        }
    }
}

/// Profiles whose SPS carries `chroma_format_idc` / bit-depth / scaling-matrix
/// fields (ITU-T H.264 § 7.3.2.1.1) — identical table to
/// `mediaway_sw::h264::sps`'s private helper of the same shape (duplicated,
/// not imported: that module's version is private to its own file).
const fn profile_has_chroma_fields(profile_idc: u8) -> bool {
    matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    )
}

/// Consume `count` `seq_scaling_list_present_flag` entries and their
/// `scaling_list()` bodies, discarding values — only bitstream alignment for
/// later fields matters here.
fn skip_scaling_lists(reader: &mut BitReader<'_>, count: usize) -> Result<(), H264Error> {
    for list_index in 0..count {
        if reader.read_bit()? != 0 {
            let size = if list_index < 6 { 16 } else { 64 };
            skip_scaling_list(reader, size)?;
        }
    }
    Ok(())
}

/// One `scaling_list(size)` body (ITU-T H.264 § 7.3.2.1.1.1).
fn skip_scaling_list(reader: &mut BitReader<'_>, size: u32) -> Result<(), H264Error> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _ in 0..size {
        if next_scale != 0 {
            let delta_scale = reader.read_se()?;
            next_scale = last_scale
                .checked_add(delta_scale)
                .and_then(|v| v.checked_add(256))
                .map(|v| v.rem_euclid(256))
                .ok_or(H264Error::FieldOverflow)?;
        }
        if next_scale != 0 {
            last_scale = next_scale;
        }
    }
    Ok(())
}

/// Derives `PicOrderCntMsb` for `pic_order_cnt_type == 0` (ITU-T H.264 § 8.2.1.1).
///
/// Given the just-parsed `pic_order_cnt_lsb` and the previous reference
/// picture's `(PicOrderCntMsb, pic_order_cnt_lsb)`. Per the spec, only
/// reference pictures update the "previous" state the caller carries forward
/// — non-reference pictures compute a POC without perpetuating it. For an
/// IDR picture, the caller passes `prev_msb = 0, prev_lsb = 0` (the spec's
/// IDR reset), so this reduces to `PicOrderCntMsb = 0`.
#[must_use]
#[allow(
    clippy::similar_names,
    reason = "prev_msb/prev_lsb name the two halves of one ITU-T H.264 § 8.2.1.1 state pair \
              (PicOrderCntMsb, pic_order_cnt_lsb) — matching, not confusable, names"
)]
pub const fn derive_pic_order_cnt_msb(
    pic_order_cnt_lsb: u32,
    prev_msb: i32,
    prev_lsb: u32,
    max_pic_order_cnt_lsb: u32,
) -> i32 {
    let half = (max_pic_order_cnt_lsb / 2) as i32;
    let lsb = pic_order_cnt_lsb as i32;
    let prev_lsb = prev_lsb as i32;
    if lsb < prev_lsb && prev_lsb - lsb >= half {
        prev_msb + max_pic_order_cnt_lsb as i32
    } else if lsb > prev_lsb && lsb - prev_lsb > half {
        prev_msb - max_pic_order_cnt_lsb as i32
    } else {
        prev_msb
    }
}

/// Build one DPB slot's `StdVideoDecodeH264ReferenceInfo` from its
/// [`crate::dpb::DpbSlot`] bookkeeping.
#[must_use]
pub fn reference_info_from_slot(
    slot: &crate::dpb::DpbSlot,
) -> native::StdVideoDecodeH264ReferenceInfo {
    let mut flags = native::StdVideoDecodeH264ReferenceInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
        __bindgen_padding_0: [0; 3],
    };
    // Progressive-only (frame_mbs_only_flag == 1): never a field picture, so
    // top_field_flag/bottom_field_flag/complementary_field_pair stay 0.
    flags.set_used_for_long_term_reference(0);
    flags.set_is_non_existing(0);
    native::StdVideoDecodeH264ReferenceInfo {
        flags,
        FrameNum: slot.frame_num as u16,
        reserved: 0,
        PicOrderCnt: [slot.pic_order_cnt, slot.pic_order_cnt],
    }
}

#[cfg(test)]
#[path = "h264_params_tests.rs"]
mod tests;
