//! H.264 sequence parameter set (SPS) parsing (ITU-T H.264 § 7.3.2.1.1).

#![forbid(unsafe_code)]

use super::bitreader::BitReader;
use super::error::H264Error;

/// Parsed fields from an H.264 sequence parameter set RBSP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sps {
    /// `profile_idc` (e.g. 66 = Baseline, 77 = Main, 100 = High).
    pub profile_idc: u8,
    /// `constraint_set0_flag` .. `constraint_set5_flag`, packed into the low 6 bits
    /// (bit 5 = `constraint_set0_flag`, .. bit 0 = `constraint_set5_flag`).
    pub constraint_flags: u8,
    /// `level_idc` (e.g. 30 = level 3.0, 41 = level 4.1).
    pub level_idc: u8,
    /// `seq_parameter_set_id`.
    pub seq_parameter_set_id: u32,
    /// Cropped picture width in luma samples (pixels).
    pub width: u32,
    /// Cropped picture height in luma samples (pixels).
    pub height: u32,
    /// `frame_mbs_only_flag`: true when the stream never codes field pictures.
    pub frame_mbs_only: bool,
    /// `chroma_format_idc` (1 = 4:2:0, the only value the decode loop in
    /// [`super::decode`] accepts; profiles without the chroma-fields block imply `1`).
    pub chroma_format_idc: u32,
    /// `log2_max_frame_num_minus4 + 4` — bit width of `frame_num` in the slice header.
    pub log2_max_frame_num: u32,
    /// `pic_order_cnt_type`. Only `0` is understood by slice header parsing today.
    pub pic_order_cnt_type: u32,
    /// `log2_max_pic_order_cnt_lsb_minus4 + 4` — bit width of `pic_order_cnt_lsb` in the
    /// slice header when `pic_order_cnt_type == 0`; `0` (unused) otherwise.
    pub log2_max_pic_order_cnt_lsb: u32,
    /// `pic_width_in_mbs_minus1 + 1` — picture width in macroblocks (uncropped).
    pub pic_width_in_mbs: u32,
    /// Picture height in macroblocks (uncropped), i.e.
    /// `PicHeightInMapUnits * (frame_mbs_only_flag ? 1 : 2)`.
    pub pic_height_in_mbs: u32,
}

/// Frame-geometry fields needed to derive [`Sps::width`] / [`Sps::height`], before
/// cropping is applied.
struct FrameGeometryFields {
    pic_width_in_mbs_minus1: u32,
    pic_height_in_map_units_minus1: u32,
    frame_mbs_only_flag: bool,
    chroma_format_idc: u32,
    separate_colour_plane_flag: bool,
}

/// `frame_cropping` rectangle fields (ITU-T H.264 § 7.4.2.1.1), in crop units (not yet
/// scaled to luma samples).
#[derive(Default)]
struct FrameCropFields {
    left: u32,
    right: u32,
    top: u32,
    bottom: u32,
}

impl Sps {
    /// Parse an SPS RBSP (NAL header byte and emulation-prevention bytes already
    /// removed, e.g. the `rbsp` of a [`super::NalUnit`] with
    /// [`super::NalUnitType::Sps`]).
    ///
    /// Parses through `frame_cropping` (enough to derive width/height/profile/level);
    /// VUI parameters (`vui_parameters_present_flag` and beyond) are not read.
    ///
    /// # Errors
    ///
    /// Returns [`H264Error`] on truncated data, an out-of-range `chroma_format_idc`, or
    /// arithmetic overflow while deriving width/height from parsed fields.
    pub fn parse(rbsp: &[u8]) -> Result<Self, H264Error> {
        let profile_idc = *rbsp.first().ok_or(H264Error::UnexpectedEof)?;
        let constraint_byte = *rbsp.get(1).ok_or(H264Error::UnexpectedEof)?;
        let level_idc = *rbsp.get(2).ok_or(H264Error::UnexpectedEof)?;
        let mut reader = BitReader::new(rbsp.get(3..).ok_or(H264Error::UnexpectedEof)?);

        let seq_parameter_set_id = reader.read_ue()?;

        let mut chroma_format_idc = 1u32;
        let mut separate_colour_plane_flag = false;
        if profile_has_chroma_fields(profile_idc) {
            chroma_format_idc = reader.read_ue()?;
            if chroma_format_idc == 3 {
                separate_colour_plane_flag = reader.read_bit()? != 0;
            }
            let _bit_depth_luma_minus8 = reader.read_ue()?;
            let _bit_depth_chroma_minus8 = reader.read_ue()?;
            let _qpprime_y_zero_transform_bypass_flag = reader.read_bit()?;
            if reader.read_bit()? != 0 {
                let scaling_list_count = if chroma_format_idc == 3 { 12 } else { 8 };
                skip_scaling_lists(&mut reader, scaling_list_count)?;
            }
        }
        if chroma_format_idc > 3 {
            return Err(H264Error::InvalidChromaFormat);
        }

        let log2_max_frame_num = reader
            .read_ue()?
            .checked_add(4)
            .ok_or(H264Error::FieldOverflow)?;
        let (pic_order_cnt_type, log2_max_pic_order_cnt_lsb) =
            read_pic_order_cnt_fields(&mut reader)?;
        let _max_num_ref_frames = reader.read_ue()?;
        let _gaps_in_frame_num_value_allowed_flag = reader.read_bit()?;

        let pic_width_in_mbs_minus1 = reader.read_ue()?;
        let pic_height_in_map_units_minus1 = reader.read_ue()?;
        let frame_mbs_only_flag = reader.read_bit()? != 0;
        if !frame_mbs_only_flag {
            let _mb_adaptive_frame_field_flag = reader.read_bit()?;
        }
        let _direct_8x8_inference_flag = reader.read_bit()?;

        let crop = if reader.read_bit()? != 0 {
            FrameCropFields {
                left: reader.read_ue()?,
                right: reader.read_ue()?,
                top: reader.read_ue()?,
                bottom: reader.read_ue()?,
            }
        } else {
            FrameCropFields::default()
        };

        let geometry = FrameGeometryFields {
            pic_width_in_mbs_minus1,
            pic_height_in_map_units_minus1,
            frame_mbs_only_flag,
            chroma_format_idc,
            separate_colour_plane_flag,
        };
        let (width, height) = compute_dimensions(&geometry, &crop)?;
        let pic_width_in_mbs = pic_width_in_mbs_minus1
            .checked_add(1)
            .ok_or(H264Error::FieldOverflow)?;
        let frame_height_factor = if frame_mbs_only_flag { 1 } else { 2 };
        let pic_height_in_mbs = pic_height_in_map_units_minus1
            .checked_add(1)
            .and_then(|v| v.checked_mul(frame_height_factor))
            .ok_or(H264Error::FieldOverflow)?;

        Ok(Self {
            profile_idc,
            constraint_flags: constraint_byte >> 2,
            level_idc,
            seq_parameter_set_id,
            width,
            height,
            frame_mbs_only: frame_mbs_only_flag,
            chroma_format_idc,
            log2_max_frame_num,
            pic_order_cnt_type,
            log2_max_pic_order_cnt_lsb,
            pic_width_in_mbs,
            pic_height_in_mbs,
        })
    }
}

/// Profiles whose SPS carries `chroma_format_idc` / bit-depth / scaling-matrix fields
/// (ITU-T H.264 § 7.3.2.1.1, the `if (profile_idc == …)` block).
const fn profile_has_chroma_fields(profile_idc: u8) -> bool {
    matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    )
}

/// Consume `pic_order_cnt_type` and whichever of its two dependent field groups apply.
/// Returns `(pic_order_cnt_type, log2_max_pic_order_cnt_lsb)`; the second value is only
/// meaningful when `pic_order_cnt_type == 0` (`0` otherwise, matching the field's absence).
fn read_pic_order_cnt_fields(reader: &mut BitReader<'_>) -> Result<(u32, u32), H264Error> {
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
            let num_ref_frames_in_pic_order_cnt_cycle = reader.read_ue()?;
            for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
                let _offset_for_ref_frame = reader.read_se()?;
            }
        }
        _ => {}
    }
    Ok((pic_order_cnt_type, log2_max_pic_order_cnt_lsb))
}

/// Consume `count` `seq_scaling_list_present_flag` entries and their `scaling_list()`
/// bodies (ITU-T H.264 § 7.3.2.1.1.1), discarding values: only bitstream alignment for
/// later fields matters here, not the scaling factors themselves.
fn skip_scaling_lists(reader: &mut BitReader<'_>, count: usize) -> Result<(), H264Error> {
    for list_index in 0..count {
        if reader.read_bit()? != 0 {
            let size = if list_index < 6 { 16 } else { 64 };
            skip_scaling_list(reader, size)?;
        }
    }
    Ok(())
}

/// Consume one `scaling_list(size)` body, per the delta-coded update rule in
/// ITU-T H.264 § 7.3.2.1.1.1: stops early once `next_scale` becomes `0`.
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

/// Derive cropped width/height from parsed geometry + crop fields
/// (ITU-T H.264 § 7.4.2.1.1), rejecting arithmetic overflow instead of wrapping.
fn compute_dimensions(
    geometry: &FrameGeometryFields,
    crop: &FrameCropFields,
) -> Result<(u32, u32), H264Error> {
    let mb_width = geometry
        .pic_width_in_mbs_minus1
        .checked_add(1)
        .ok_or(H264Error::FieldOverflow)?;
    let map_units_height = geometry
        .pic_height_in_map_units_minus1
        .checked_add(1)
        .ok_or(H264Error::FieldOverflow)?;
    let frame_height_factor = if geometry.frame_mbs_only_flag { 1 } else { 2 };
    let mb_height = map_units_height
        .checked_mul(frame_height_factor)
        .ok_or(H264Error::FieldOverflow)?;

    let raw_width = mb_width.checked_mul(16).ok_or(H264Error::FieldOverflow)?;
    let raw_height = mb_height.checked_mul(16).ok_or(H264Error::FieldOverflow)?;

    let chroma_array_type = if geometry.separate_colour_plane_flag {
        0
    } else {
        geometry.chroma_format_idc
    };
    let (sub_width_c, sub_height_c) = match geometry.chroma_format_idc {
        2 => (2, 1),
        3 => (1, 1),
        _ => (2, 2), // 0 (monochrome) and 1 (4:2:0) both unused/2 below
    };
    let (crop_unit_x, crop_unit_y) = if chroma_array_type == 0 {
        (1, frame_height_factor)
    } else {
        (sub_width_c, sub_height_c * frame_height_factor)
    };

    let left_right_sum = crop
        .left
        .checked_add(crop.right)
        .ok_or(H264Error::FieldOverflow)?;
    let top_bottom_sum = crop
        .top
        .checked_add(crop.bottom)
        .ok_or(H264Error::FieldOverflow)?;
    let crop_width = left_right_sum
        .checked_mul(crop_unit_x)
        .ok_or(H264Error::FieldOverflow)?;
    let crop_height = top_bottom_sum
        .checked_mul(crop_unit_y)
        .ok_or(H264Error::FieldOverflow)?;

    let width = raw_width
        .checked_sub(crop_width)
        .ok_or(H264Error::FieldOverflow)?;
    let height = raw_height
        .checked_sub(crop_height)
        .ok_or(H264Error::FieldOverflow)?;
    Ok((width, height))
}

#[cfg(test)]
#[path = "sps_tests.rs"]
mod tests;
