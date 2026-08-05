//! `profile_tier_level()` (ITU-T H.265 § 7.3.3) parsing and the
//! raw-to-`StdVideoH265LevelIdc` conversion it needs — split out of
//! `hevc_params.rs` purely to stay under this workspace's 1000-line-per-file
//! rule (mirrors `windows/d3d12_video_decode.rs`'s own `mod`-per-file split
//! for the same reason, not a different architecture). `HevcSps::parse`
//! calls [`parse_profile_tier_level`] inline as one step of its own linear
//! SPS parse; `HevcSps::to_std_profile_tier_level` calls [`std_level_idc`].

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "every count here comes from an Exp-Golomb/fixed-width HEVC syntax element, always \
              small in practice — mirrors hevc_params.rs's own identical allow"
)]

use mediaway_sw::h264::BitReader;
use vulkanalia::vk::video as native;

use super::HevcParamError;

/// The subset of `profile_tier_level()`'s general-profile fields this crate's
/// decode session needs — see [`parse_profile_tier_level`].
#[allow(
    clippy::struct_field_names,
    reason = "every field name is the real ITU-T H.265 profile_tier_level() syntax element name \
              (general_profile_idc, general_tier_flag, ...) — renaming to avoid the shared \
              `general` prefix would obscure the 1:1 spec mapping, same reasoning as HevcSps's \
              identical field names"
)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is a real, independent ITU-T H.265 profile_tier_level() constraint flag — \
              same reasoning as HevcSps's identical allow"
)]
pub(super) struct ProfileTierLevel {
    pub(super) general_profile_idc: u8,
    pub(super) general_level_idc: u8,
    pub(super) general_tier_flag: bool,
    pub(super) general_progressive_source_flag: bool,
    pub(super) general_interlaced_source_flag: bool,
    pub(super) general_non_packed_constraint_flag: bool,
    pub(super) general_frame_only_constraint_flag: bool,
}

/// `profile_tier_level(profilePresentFlag=1, maxNumSubLayersMinus1)` (ITU-T
/// H.265 § 7.3.3), general-only (no sub-layer profile/level, since
/// `sps_max_sub_layers_minus1 == 0` is required by `HevcSps::parse`).
pub(super) fn parse_profile_tier_level(
    reader: &mut BitReader<'_>,
    max_num_sub_layers_minus1: u32,
) -> Result<ProfileTierLevel, HevcParamError> {
    let _general_profile_space = reader.read_bits(2)?;
    let general_tier_flag = reader.read_bit()? != 0;
    let general_profile_idc = reader.read_bits(5)? as u8;
    for _ in 0..32 {
        let _general_profile_compatibility_flag = reader.read_bit()?;
    }
    let general_progressive_source_flag = reader.read_bit()? != 0;
    let general_interlaced_source_flag = reader.read_bit()? != 0;
    let general_non_packed_constraint_flag = reader.read_bit()? != 0;
    let general_frame_only_constraint_flag = reader.read_bit()? != 0;
    // 43 general reserved/constraint bits + 1 (44 total, per the spec's
    // profile_idc-conditional block collapsing to a flat 44-bit reserved
    // field for the profiles this crate accepts).
    let _ = reader.read_bits(32)?;
    let _ = reader.read_bits(12)?;
    let general_level_idc = reader.read_bits(8)? as u8;
    for _ in 0..max_num_sub_layers_minus1 {
        // Unreachable this round (max_num_sub_layers_minus1 == 0 always,
        // enforced by the caller), kept for structural completeness.
        let _sub_layer_profile_present_flag = reader.read_bit()?;
        let _sub_layer_level_present_flag = reader.read_bit()?;
    }
    Ok(ProfileTierLevel {
        general_profile_idc,
        general_level_idc,
        general_tier_flag,
        general_progressive_source_flag,
        general_interlaced_source_flag,
        general_non_packed_constraint_flag,
        general_frame_only_constraint_flag,
    })
}

/// Converts a raw ITU-T H.265 `general_level_idc` syntax-element byte (Table
/// A.1: `general_level_idc == 30 * level_number`, e.g. `93` for Level 3.1)
/// into the small sequential ordinal `StdVideoH265LevelIdc` actually expects
/// (`STD_VIDEO_H265_LEVEL_IDC_3_1 == 4`) — **not** a raw
/// reinterpret/`i32::from` cast, unlike `general_profile_idc` (whose few
/// legal raw values — `1`/`2`/`3`/`4`/`9` — already coincide with their
/// `StdVideoH265ProfileIdc` ordinal by coincidence of how few profiles
/// exist). This crate's original HEVC decode attempt cast the raw byte
/// directly; confirmed against `FFmpeg`'s real `libavcodec/vulkan_hevc.c`,
/// which always routes through its own `ff_vk_h265_level_to_vk` lookup
/// instead. An unrecognized raw value (reserved/future level) maps to the
/// smallest known level at or above it, never silently rounding down below
/// what was actually signaled.
#[must_use]
pub(super) fn std_level_idc(general_level_idc: u8) -> native::StdVideoH265LevelIdc {
    const LEVELS: [(u8, native::StdVideoH265LevelIdc); 13] = [
        (30, native::STD_VIDEO_H265_LEVEL_IDC_1_0),
        (60, native::STD_VIDEO_H265_LEVEL_IDC_2_0),
        (63, native::STD_VIDEO_H265_LEVEL_IDC_2_1),
        (90, native::STD_VIDEO_H265_LEVEL_IDC_3_0),
        (93, native::STD_VIDEO_H265_LEVEL_IDC_3_1),
        (120, native::STD_VIDEO_H265_LEVEL_IDC_4_0),
        (123, native::STD_VIDEO_H265_LEVEL_IDC_4_1),
        (150, native::STD_VIDEO_H265_LEVEL_IDC_5_0),
        (153, native::STD_VIDEO_H265_LEVEL_IDC_5_1),
        (156, native::STD_VIDEO_H265_LEVEL_IDC_5_2),
        (180, native::STD_VIDEO_H265_LEVEL_IDC_6_0),
        (183, native::STD_VIDEO_H265_LEVEL_IDC_6_1),
        (186, native::STD_VIDEO_H265_LEVEL_IDC_6_2),
    ];
    LEVELS
        .iter()
        .find(|(raw, _)| *raw == general_level_idc)
        .or_else(|| LEVELS.iter().find(|(raw, _)| *raw >= general_level_idc))
        .map_or(native::STD_VIDEO_H265_LEVEL_IDC_6_2, |(_, level)| *level)
}

#[cfg(test)]
#[path = "hevc_ptl_tests.rs"]
mod tests;
