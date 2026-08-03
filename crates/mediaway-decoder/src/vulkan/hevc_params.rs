//! HEVC 2-byte NAL header parsing, VPS/SPS/PPS parsing, and
//! `StdVideoH265*`/`StdVideoDecodeH265*` construction.
//!
//! Reuses [`mediaway_sw::h264::{BitReader, split_annex_b}`] (both codec-agnostic
//! — Annex-B start-code framing does not interpret `nal_unit_type`, and
//! bit-level RBSP reading is generic) but **not** `mediaway_sw::h264::NalUnit`,
//! whose header parse is H.264's 1-byte layout — HEVC's own NAL header is 2
//! bytes (`forbidden_zero_bit`, 6-bit `nal_unit_type`, 6-bit `nuh_layer_id`,
//! 3-bit `nuh_temporal_id_plus1`), parsed fresh here. Per `adr/0001`'s
//! "Bitstream-parser reuse" section.
//!
//! Struct shapes (`StdVideoH265SequenceParameterSet`,
//! `StdVideoH265PictureParameterSet`, `StdVideoH265VideoParameterSet`,
//! `StdVideoH265ProfileTierLevel`, `StdVideoH265DecPicBufMgr`) are confirmed
//! directly against `vulkanalia` 0.35's vendored source (not just inferred
//! from the H.264 pattern, per this round's task) and mirror
//! `mediaway-encoder-vulkan::hevc_params`'s own field list (that crate builds
//! the same structs in the encode direction, with fixed constants; this one
//! parses them from real bitstream bytes instead).
//!
//! Scope, matching `adr/0001` and this crate's H.264 module: 4:2:0 8-bit
//! only, progressive only, single tile, no WPP
//! (`entropy_coding_sync_enabled_flag`), Main profile. Short-term RPS parsing
//! supports only the non-predicted case (`inter_ref_pic_set_prediction_flag
//! == 0`) — the common case real encoders emit for an SPS-level RPS list;
//! predicted RPS entries are rejected as `Unsupported` (same "sliding-window
//! equivalent, not full spec generality" cut as H.264's `dpb.rs`).

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "every count here comes from an Exp-Golomb-decoded HEVC syntax element, always \
              small in practice — narrowing casts into the small Std*/vulkanalia field widths \
              mirror this crate's own h264_params.rs allow for the identical shape"
)]

use mediaway_sw::h264::{BitReader, H264Error};
use thiserror::Error;
use vulkanalia::vk::video as native;

/// Errors from parsing an HEVC VPS/SPS/PPS/slice-segment-header, or a syntax
/// element this crate's scope does not support.
///
/// Crate-internal, mirrors `h264_params::H264ParamError`'s role — wrapped
/// into [`crate::vulkan::session::VulkanDecodeError`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HevcParamError {
    /// Truncated data or an Exp-Golomb/arithmetic overflow while reading.
    #[error(transparent)]
    Bitstream(#[from] H264Error),
    /// A syntax element this crate's scope does not decode (see the module
    /// doc's scope list).
    #[error("unsupported HEVC syntax: {reason}")]
    Unsupported {
        /// Human-readable reason, always a `'static` literal at call sites.
        reason: &'static str,
    },
}

/// HEVC NAL unit type (`nal_unit_type`, ITU-T H.265 Table 7-1) — only the
/// values this crate's decode path checks get named variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HevcNalUnitType {
    /// Coded slice segment, trailing picture, not a reference (`TRAIL_N`, 0)
    /// or a reference (`TRAIL_R`, 1) — the common non-IRAP inter picture
    /// types real encoders emit.
    Trail,
    /// IDR picture, no leading pictures (`IDR_W_RADL` 19 / `IDR_N_LP` 20).
    Idr,
    /// CRA picture (`CRA_NUT`, 21) — an intra random-access point that is
    /// not an IDR (may have leading pictures); this crate treats it as an
    /// intra picture for decode purposes, matching how a CRA's own slice
    /// segment header carries the same "no POC/RPS fields" IRAP shape as
    /// IDR — see [`HevcSliceSegmentHeader::parse`].
    Cra,
    /// Video parameter set (32).
    Vps,
    /// Sequence parameter set (33).
    Sps,
    /// Picture parameter set (34).
    Pps,
    /// Any other type value (SEI, AUD, other slice types, extensions).
    Other(u8),
}

impl HevcNalUnitType {
    #[must_use]
    const fn from_u8(value: u8) -> Self {
        match value {
            0..=9 => Self::Trail,
            19 | 20 => Self::Idr,
            21 => Self::Cra,
            32 => Self::Vps,
            33 => Self::Sps,
            34 => Self::Pps,
            other => Self::Other(other),
        }
    }

    /// Whether this NAL unit type is `IDR_W_RADL`/`IDR_N_LP` — **only** these
    /// two types skip the slice segment header's POC-LSB/RPS fields entirely
    /// (ITU-T H.265 § 7.3.6.1's `if (nal_unit_type != IDR_W_RADL &&
    /// nal_unit_type != IDR_N_LP)` guard). `CRA_NUT` is also an intra
    /// random-access point but is **not** exempt from POC/RPS signaling —
    /// conflating the two was a real mistake caught while writing
    /// [`crate::vulkan::hevc_slice::HevcSliceSegmentHeader::parse`].
    #[must_use]
    pub(crate) const fn is_idr(self) -> bool {
        matches!(self, Self::Idr)
    }
}

/// One parsed HEVC NAL unit: 2-byte header fields plus RBSP payload.
///
/// Emulation-prevention bytes are already removed by the caller. This crate
/// does **not** reuse `mediaway_sw::h264`'s emulation-prevention removal
/// (that lives behind `NalUnit::parse`, which assumes a 1-byte H.264 header);
/// instead this type re-implements the (codec-agnostic) emulation-prevention
/// removal directly.
#[derive(Debug, Clone)]
pub struct HevcNalUnit {
    /// Decoded NAL unit type.
    pub unit_type: HevcNalUnitType,
    /// `nuh_layer_id` (always `0` for a single-layer, non-scalable stream —
    /// this crate's scope; a nonzero value is rejected by
    /// [`HevcNalUnit::parse`]).
    pub nuh_layer_id: u8,
    /// `nuh_temporal_id_plus1 - 1` (`TemporalId`).
    pub temporal_id: u8,
    /// Payload after the 2-byte NAL header, with emulation-prevention bytes
    /// removed.
    pub rbsp: Vec<u8>,
}

impl HevcNalUnit {
    /// Parse one NAL unit's 2-byte header + de-emulated RBSP from `data`,
    /// which must start at the first header byte (no start code / length
    /// prefix), e.g. one element of
    /// [`mediaway_sw::h264::split_annex_b`].
    ///
    /// # Errors
    ///
    /// [`HevcParamError::Bitstream`] if `data` is shorter than 2 bytes.
    /// [`HevcParamError::Unsupported`] if `nuh_layer_id != 0` (multi-layer/
    /// scalable HEVC is out of scope).
    pub fn parse(data: &[u8]) -> Result<Self, HevcParamError> {
        let first = *data.first().ok_or(H264Error::UnexpectedEof)?;
        let second = *data.get(1).ok_or(H264Error::UnexpectedEof)?;
        // forbidden_zero_bit (1 bit) + nal_unit_type (6 bits) + nuh_layer_id
        // high bit (1 bit), all in the first byte; nuh_layer_id low 5 bits +
        // nuh_temporal_id_plus1 (3 bits) in the second.
        let nal_unit_type = (first >> 1) & 0x3F;
        let nuh_layer_id = ((first & 0x1) << 5) | (second >> 3);
        let temporal_id_plus1 = second & 0x7;
        if nuh_layer_id != 0 {
            return Err(HevcParamError::Unsupported {
                reason: "nuh_layer_id != 0 (multi-layer/scalable HEVC) is not supported",
            });
        }
        if temporal_id_plus1 == 0 {
            return Err(H264Error::UnexpectedEof.into());
        }
        let rbsp = remove_emulation_prevention(data.get(2..).ok_or(H264Error::UnexpectedEof)?);
        Ok(Self {
            unit_type: HevcNalUnitType::from_u8(nal_unit_type),
            nuh_layer_id,
            temporal_id: temporal_id_plus1 - 1,
            rbsp,
        })
    }
}

/// Remove `emulation_prevention_three_byte` — identical rule to H.264
/// (ITU-T H.265 § 7.3.1.1 uses the same `00 00 03` convention), duplicated
/// here (not imported) since `mediaway_sw::h264::nal`'s version is a private
/// function of that module.
fn remove_emulation_prevention(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut zero_run = 0u32;
    for &byte in data {
        if zero_run >= 2 && byte == 0x03 {
            zero_run = 0;
            continue;
        }
        out.push(byte);
        zero_run = if byte == 0 { zero_run + 1 } else { 0 };
    }
    out
}

/// Parsed HEVC SPS fields this crate's decode session needs.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is a real, independent ITU-T H.265 SPS flag that must be echoed into \
              StdVideoH265SequenceParameterSet exactly as signaled — collapsing them into an enum \
              would obscure the 1:1 spec mapping for no real benefit (they are read/written \
              independently, never as a combined state machine)"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HevcSps {
    /// `sps_video_parameter_set_id`.
    pub sps_video_parameter_set_id: u8,
    /// `sps_seq_parameter_set_id`.
    pub sps_seq_parameter_set_id: u8,
    /// `pic_width_in_luma_samples`.
    pub pic_width_in_luma_samples: u32,
    /// `pic_height_in_luma_samples`.
    pub pic_height_in_luma_samples: u32,
    /// `log2_max_pic_order_cnt_lsb_minus4 + 4`.
    pub log2_max_pic_order_cnt_lsb: u32,
    /// `sps_max_dec_pic_buffering_minus1[sps_max_sub_layers_minus1] + 1` —
    /// sizes this session's DPB (single sub-layer only, see scope cut).
    pub max_dec_pic_buffering: u32,
    /// `log2_min_luma_coding_block_size_minus3 + 3`.
    pub log2_min_cb_size: u32,
    /// `log2_diff_max_min_luma_coding_block_size`.
    pub log2_diff_max_min_cb_size: u32,
    /// `log2_min_luma_transform_block_size_minus2 + 2`.
    pub log2_min_tb_size: u32,
    /// `log2_diff_max_min_luma_transform_block_size`.
    pub log2_diff_max_min_tb_size: u32,
    /// `max_transform_hierarchy_depth_inter`.
    pub max_transform_hierarchy_depth_inter: u32,
    /// `max_transform_hierarchy_depth_intra`.
    pub max_transform_hierarchy_depth_intra: u32,
    /// `general_profile_idc` (from `profile_tier_level`).
    pub general_profile_idc: u8,
    /// `general_level_idc`.
    pub general_level_idc: u8,
    /// `amp_enabled_flag` — must be echoed into `StdVideoH265SequenceParameterSet`
    /// exactly as signaled: it changes how a real encoder binarizes inter
    /// `part_mode`, so a mismatch desyncs the hardware's own CABAC parser
    /// from the very first inter CU (real hardware finding — see `adr/0001`'s
    /// 2026-07-30 HEVC addendum).
    pub amp_enabled_flag: bool,
    /// `sample_adaptive_offset_enabled_flag` — same "must be echoed exactly"
    /// requirement as `amp_enabled_flag`. This one, unlike `amp_enabled_flag`,
    /// also affects intra-only slices: real HEVC encoders commonly enable SAO
    /// by default, and `slice_sao_luma_flag`/`slice_sao_chroma_flag` are
    /// present in **every** slice's header (any slice type, including IDR)
    /// whenever this flag is set. Leaving this always-`false` in
    /// `StdVideoH265SequenceParameterSet` (this crate's first HEVC hardware
    /// bug) desyncs the driver's own slice-header parse before the first CTU
    /// even starts, on any stream a real encoder produced with SAO on.
    pub sample_adaptive_offset_enabled_flag: bool,
    /// `sps_temporal_mvp_enabled_flag` — gates `slice_temporal_mvp_enabled_flag`
    /// in the slice header (non-IDR only); must be echoed exactly for the
    /// same reason as `amp_enabled_flag`/`sample_adaptive_offset_enabled_flag`.
    pub sps_temporal_mvp_enabled_flag: bool,
    /// `strong_intra_smoothing_enabled_flag` — does not add slice-header
    /// syntax bits, but does change decoded intra-prediction *sample values*
    /// for 32x32 intra blocks; echoed for output correctness, not parser sync.
    pub strong_intra_smoothing_enabled_flag: bool,
}

impl HevcSps {
    /// Parse an SPS RBSP (2-byte NAL header already stripped, e.g. via
    /// [`HevcNalUnit::parse`]).
    ///
    /// # Errors
    ///
    /// [`HevcParamError::Unsupported`] when `chroma_format_idc != 1`,
    /// `separate_colour_plane_flag == 1`, `sps_max_sub_layers_minus1 != 0`
    /// (temporal scalability out of scope), `scaling_list_enabled_flag == 1`,
    /// `pcm_enabled_flag == 1` (not implemented this round — see
    /// [`HevcSps::amp_enabled_flag`]'s sibling fields for the flags that
    /// *are* supported and must be echoed exactly), or
    /// `num_short_term_ref_pic_sets > 0`/`long_term_ref_pics_present_flag ==
    /// 1` (SPS-level RPS lists are not parsed this round — see the module
    /// doc; a stream needing them is rejected rather than silently
    /// mis-parsed). Other [`HevcParamError::Bitstream`] variants on
    /// truncated/overflowing data.
    #[allow(
        clippy::too_many_lines,
        reason = "linear ITU-T H.265 § 7.3.2.2.1 syntax-element sequence through profile_tier_level \
                  and the fields this crate's decode session needs — splitting further would just \
                  move consecutive reads of the same RBSP into a same-file helper"
    )]
    #[allow(
        clippy::similar_names,
        reason = "log2_min_cb_size/log2_min_tb_size and log2_diff_max_min_cb_size/log2_diff_max_min_tb_size \
                  are the real ITU-T H.265 § 7.3.2.2.1 syntax element names (coding-block vs \
                  transform-block size) — renaming to look less similar would obscure the spec mapping"
    )]
    pub fn parse(rbsp: &[u8]) -> Result<Self, HevcParamError> {
        let mut reader = BitReader::new(rbsp);
        let sps_video_parameter_set_id = reader.read_bits(4)? as u8;
        let sps_max_sub_layers_minus1 = reader.read_bits(3)?;
        if sps_max_sub_layers_minus1 != 0 {
            return Err(HevcParamError::Unsupported {
                reason: "sps_max_sub_layers_minus1 != 0 (temporal scalability) is not supported",
            });
        }
        let _sps_temporal_id_nesting_flag = reader.read_bit()?;
        let (general_profile_idc, general_level_idc) =
            parse_profile_tier_level(&mut reader, sps_max_sub_layers_minus1)?;

        let sps_seq_parameter_set_id = reader.read_ue()? as u8;
        let chroma_format_idc = reader.read_ue()?;
        if chroma_format_idc == 3 {
            let _separate_colour_plane_flag = reader.read_bit()?;
        }
        if chroma_format_idc != 1 {
            return Err(HevcParamError::Unsupported {
                reason: "only chroma_format_idc == 1 (4:2:0) is supported",
            });
        }
        let pic_width_in_luma_samples = reader.read_ue()?;
        let pic_height_in_luma_samples = reader.read_ue()?;
        if reader.read_bit()? != 0 {
            // conformance_window_flag
            let _conf_win_left_offset = reader.read_ue()?;
            let _conf_win_right_offset = reader.read_ue()?;
            let _conf_win_top_offset = reader.read_ue()?;
            let _conf_win_bottom_offset = reader.read_ue()?;
        }
        let _bit_depth_luma_minus8 = reader.read_ue()?;
        let _bit_depth_chroma_minus8 = reader.read_ue()?;
        let log2_max_pic_order_cnt_lsb = reader
            .read_ue()?
            .checked_add(4)
            .ok_or(H264Error::FieldOverflow)?;

        let sps_sub_layer_ordering_info_present_flag = reader.read_bit()? != 0;
        // Only one sub-layer (checked above) — read exactly one ordering
        // triple, or all `sps_max_sub_layers_minus1 + 1` if the "present"
        // flag requests every sub-layer's own values (same count here since
        // there is only one sub-layer either way).
        let start = if sps_sub_layer_ordering_info_present_flag {
            0
        } else {
            sps_max_sub_layers_minus1
        };
        let mut max_dec_pic_buffering = 1u32;
        for _ in start..=sps_max_sub_layers_minus1 {
            max_dec_pic_buffering = reader
                .read_ue()?
                .checked_add(1)
                .ok_or(H264Error::FieldOverflow)?;
            let _sps_max_num_reorder_pics = reader.read_ue()?;
            let _sps_max_latency_increase_plus1 = reader.read_ue()?;
        }

        let log2_min_cb_size = reader
            .read_ue()?
            .checked_add(3)
            .ok_or(H264Error::FieldOverflow)?;
        let log2_diff_max_min_cb_size = reader.read_ue()?;
        let log2_min_tb_size = reader
            .read_ue()?
            .checked_add(2)
            .ok_or(H264Error::FieldOverflow)?;
        let log2_diff_max_min_tb_size = reader.read_ue()?;
        let max_transform_hierarchy_depth_inter = reader.read_ue()?;
        let max_transform_hierarchy_depth_intra = reader.read_ue()?;

        if reader.read_bit()? != 0 {
            return Err(HevcParamError::Unsupported {
                reason: "scaling_list_enabled_flag == 1 is not supported",
            });
        }
        // These two must be stored, not discarded — see the struct fields'
        // own doc comments for why (a real hardware bug this crate's first
        // HEVC decode attempt hit: `StdVideoH265SequenceParameterSet` always
        // reported both as `0`/disabled regardless of what a real encoder
        // actually signaled, desyncing the driver's own bitstream parser).
        let amp_enabled_flag = reader.read_bit()? != 0;
        let sample_adaptive_offset_enabled_flag = reader.read_bit()? != 0;
        let pcm_enabled_flag = reader.read_bit()? != 0;
        if pcm_enabled_flag {
            return Err(HevcParamError::Unsupported {
                reason: "pcm_enabled_flag == 1 is not supported this round",
            });
        }

        let num_short_term_ref_pic_sets = reader.read_ue()?;
        if num_short_term_ref_pic_sets > 0 {
            return Err(HevcParamError::Unsupported {
                reason: "SPS-level num_short_term_ref_pic_sets > 0 is not supported this round",
            });
        }
        let long_term_ref_pics_present_flag = reader.read_bit()? != 0;
        if long_term_ref_pics_present_flag {
            return Err(HevcParamError::Unsupported {
                reason: "long_term_ref_pics_present_flag == 1 is not supported this round",
            });
        }
        let sps_temporal_mvp_enabled_flag = reader.read_bit()? != 0;
        let strong_intra_smoothing_enabled_flag = reader.read_bit()? != 0;
        // `vui_parameters_present_flag` (+ VUI body) and
        // `sps_extension_present_flag` follow — this crate's decode path does
        // not need them (VUI/extensions do not change how the hardware's own
        // bitstream parser reads slice/CTU syntax), so parsing intentionally
        // stops here, matching `mediaway_sw::h264::Sps::parse`'s own "parse
        // through what's needed" convention.

        Ok(Self {
            sps_video_parameter_set_id,
            sps_seq_parameter_set_id,
            pic_width_in_luma_samples,
            pic_height_in_luma_samples,
            log2_max_pic_order_cnt_lsb,
            max_dec_pic_buffering,
            log2_min_cb_size,
            log2_diff_max_min_cb_size,
            log2_min_tb_size,
            log2_diff_max_min_tb_size,
            max_transform_hierarchy_depth_inter,
            max_transform_hierarchy_depth_intra,
            general_profile_idc,
            general_level_idc,
            amp_enabled_flag,
            sample_adaptive_offset_enabled_flag,
            sps_temporal_mvp_enabled_flag,
            strong_intra_smoothing_enabled_flag,
        })
    }

    /// Builds the `StdVideoH265ProfileTierLevel` this SPS parsed.
    #[must_use]
    pub fn to_std_profile_tier_level(&self) -> native::StdVideoH265ProfileTierLevel {
        let mut flags = native::StdVideoH265ProfileTierLevelFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
            __bindgen_padding_0: [0; 3],
        };
        flags.set_general_progressive_source_flag(1);
        flags.set_general_frame_only_constraint_flag(1);
        native::StdVideoH265ProfileTierLevel {
            flags,
            general_profile_idc: native::StdVideoH265ProfileIdc(i32::from(
                self.general_profile_idc,
            )),
            general_level_idc: native::StdVideoH265LevelIdc(i32::from(self.general_level_idc)),
        }
    }

    /// Builds the `StdVideoH265DecPicBufMgr` this SPS parsed (single
    /// sub-layer, see the scope cut in [`HevcSps::parse`]).
    #[must_use]
    pub const fn to_std_dec_pic_buf_mgr(&self) -> native::StdVideoH265DecPicBufMgr {
        let mut max_dec_pic_buffering_minus1 = [0u8; 7];
        max_dec_pic_buffering_minus1[0] = (self.max_dec_pic_buffering - 1) as u8;
        native::StdVideoH265DecPicBufMgr {
            max_latency_increase_plus1: [0; 7],
            max_dec_pic_buffering_minus1,
            max_num_reorder_pics: [0; 7],
        }
    }

    /// Builds the `StdVideoH265SequenceParameterSet` this SPS's fields
    /// describe. `profile_tier_level`/`dec_pic_buf_mgr` must outlive the
    /// returned struct (raw pointer fields — same pattern as
    /// `mediaway-encoder-vulkan::hevc_params::build_sps`).
    #[must_use]
    pub fn to_std(
        &self,
        profile_tier_level: &native::StdVideoH265ProfileTierLevel,
        dec_pic_buf_mgr: &native::StdVideoH265DecPicBufMgr,
    ) -> native::StdVideoH265SequenceParameterSet {
        let mut flags = native::StdVideoH265SpsFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        };
        // Every flag here must echo exactly what this SPS actually signaled —
        // see the struct fields' own doc comments for the real hardware bug
        // this fixes (a mismatched `sample_adaptive_offset_enabled_flag`
        // desyncs the driver's own slice-header parser on any real encoder
        // output that turns SAO on, which most do by default).
        flags.set_amp_enabled_flag(u32::from(self.amp_enabled_flag));
        flags.set_sample_adaptive_offset_enabled_flag(u32::from(
            self.sample_adaptive_offset_enabled_flag,
        ));
        flags.set_sps_temporal_mvp_enabled_flag(u32::from(self.sps_temporal_mvp_enabled_flag));
        flags.set_strong_intra_smoothing_enabled_flag(u32::from(
            self.strong_intra_smoothing_enabled_flag,
        ));
        native::StdVideoH265SequenceParameterSet {
            flags,
            chroma_format_idc: native::STD_VIDEO_H265_CHROMA_FORMAT_IDC_420,
            pic_width_in_luma_samples: self.pic_width_in_luma_samples,
            pic_height_in_luma_samples: self.pic_height_in_luma_samples,
            sps_video_parameter_set_id: self.sps_video_parameter_set_id,
            sps_max_sub_layers_minus1: 0,
            sps_seq_parameter_set_id: self.sps_seq_parameter_set_id,
            bit_depth_luma_minus8: 0,
            bit_depth_chroma_minus8: 0,
            log2_max_pic_order_cnt_lsb_minus4: (self.log2_max_pic_order_cnt_lsb - 4) as u8,
            log2_min_luma_coding_block_size_minus3: (self.log2_min_cb_size - 3) as u8,
            log2_diff_max_min_luma_coding_block_size: self.log2_diff_max_min_cb_size as u8,
            log2_min_luma_transform_block_size_minus2: (self.log2_min_tb_size - 2) as u8,
            log2_diff_max_min_luma_transform_block_size: self.log2_diff_max_min_tb_size as u8,
            max_transform_hierarchy_depth_inter: self.max_transform_hierarchy_depth_inter as u8,
            max_transform_hierarchy_depth_intra: self.max_transform_hierarchy_depth_intra as u8,
            num_short_term_ref_pic_sets: 0,
            num_long_term_ref_pics_sps: 0,
            pcm_sample_bit_depth_luma_minus1: 0,
            pcm_sample_bit_depth_chroma_minus1: 0,
            log2_min_pcm_luma_coding_block_size_minus3: 0,
            log2_diff_max_min_pcm_luma_coding_block_size: 0,
            reserved1: 0,
            reserved2: 0,
            palette_max_size: 0,
            delta_palette_max_predictor_size: 0,
            motion_vector_resolution_control_idc: 0,
            sps_num_palette_predictor_initializers_minus1: 0,
            conf_win_left_offset: 0,
            conf_win_right_offset: 0,
            conf_win_top_offset: 0,
            conf_win_bottom_offset: 0,
            pProfileTierLevel: profile_tier_level,
            pDecPicBufMgr: dec_pic_buf_mgr,
            pScalingLists: core::ptr::null(),
            pShortTermRefPicSet: core::ptr::null(),
            pLongTermRefPicsSps: core::ptr::null(),
            pSequenceParameterSetVui: core::ptr::null(),
            pPredictorPaletteEntries: core::ptr::null(),
        }
    }
}

/// `profile_tier_level(profilePresentFlag=1, maxNumSubLayersMinus1)` (ITU-T
/// H.265 § 7.3.3), general-only (no sub-layer profile/level, since
/// `sps_max_sub_layers_minus1 == 0` is required by [`HevcSps::parse`]).
/// Returns `(general_profile_idc, general_level_idc)`.
fn parse_profile_tier_level(
    reader: &mut BitReader<'_>,
    max_num_sub_layers_minus1: u32,
) -> Result<(u8, u8), HevcParamError> {
    let _general_profile_space = reader.read_bits(2)?;
    let _general_tier_flag = reader.read_bit()?;
    let general_profile_idc = reader.read_bits(5)? as u8;
    for _ in 0..32 {
        let _general_profile_compatibility_flag = reader.read_bit()?;
    }
    let _general_progressive_source_flag = reader.read_bit()?;
    let _general_interlaced_source_flag = reader.read_bit()?;
    let _general_non_packed_constraint_flag = reader.read_bit()?;
    let _general_frame_only_constraint_flag = reader.read_bit()?;
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
    Ok((general_profile_idc, general_level_idc))
}

/// Parsed HEVC PPS fields this crate's decode session needs.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is a real, independent ITU-T H.265 PPS flag that must be echoed into \
              StdVideoH265PictureParameterSet exactly as signaled — same reasoning as HevcSps's \
              identical allow"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HevcPps {
    /// `pps_pic_parameter_set_id`.
    pub pps_pic_parameter_set_id: u8,
    /// `pps_seq_parameter_set_id`.
    pub pps_seq_parameter_set_id: u8,
    /// `output_flag_present_flag` — gates `pic_output_flag` in the slice
    /// segment header (needed by [`crate::vulkan::hevc_slice`] to stay bit-aligned).
    pub output_flag_present_flag: bool,
    /// `num_extra_slice_header_bits` — count of `slice_reserved_flag[i]` bits
    /// to skip in the slice segment header (needed by [`crate::vulkan::hevc_slice`]).
    pub num_extra_slice_header_bits: u32,
    /// `num_ref_idx_l0_default_active_minus1 + 1`.
    pub num_ref_idx_l0_default_active: u32,
    /// `num_ref_idx_l1_default_active_minus1 + 1`.
    pub num_ref_idx_l1_default_active: u32,
    /// `init_qp_minus26 + 26`.
    pub init_qp: i32,
    /// `dependent_slice_segments_enabled_flag`. Must be echoed into
    /// `StdVideoH265PictureParameterSet` exactly, same reasoning as every
    /// other field below — see [`HevcSps::amp_enabled_flag`]'s doc for the
    /// real hardware bug this class of field discovered.
    pub dependent_slice_segments_enabled_flag: bool,
    /// `sign_data_hiding_enabled_flag`.
    pub sign_data_hiding_enabled_flag: bool,
    /// `cabac_init_present_flag`.
    pub cabac_init_present_flag: bool,
    /// `constrained_intra_pred_flag`.
    pub constrained_intra_pred_flag: bool,
    /// `transform_skip_enabled_flag` — gates a per-TU `transform_skip_flag`
    /// bit; a mismatch here desyncs CABAC parsing on the first coded TU.
    pub transform_skip_enabled_flag: bool,
    /// `cu_qp_delta_enabled_flag` — gates per-CU-group `cu_qp_delta_abs`/
    /// `cu_qp_delta_sign_flag` bits; a mismatch desyncs CABAC parsing on the
    /// first CU.
    pub cu_qp_delta_enabled_flag: bool,
    /// `diff_cu_qp_delta_depth` — only meaningful when
    /// `cu_qp_delta_enabled_flag` is set; `0` otherwise.
    pub diff_cu_qp_delta_depth: u32,
    /// `pps_cb_qp_offset` (`se(v)`).
    pub pps_cb_qp_offset: i32,
    /// `pps_cr_qp_offset` (`se(v)`).
    pub pps_cr_qp_offset: i32,
    /// `pps_slice_chroma_qp_offsets_present_flag` — gates
    /// `slice_cb_qp_offset`/`slice_cr_qp_offset` bits in **every** slice
    /// header (any slice type); a mismatch desyncs the slice header parse
    /// itself, before any CTU data.
    pub pps_slice_chroma_qp_offsets_present_flag: bool,
    /// `weighted_pred_flag` (P slices only — irrelevant to this crate's
    /// current all-intra decode path, stored for completeness/future use).
    pub weighted_pred_flag: bool,
    /// `weighted_bipred_flag` (B slices only).
    pub weighted_bipred_flag: bool,
    /// `transquant_bypass_enabled_flag` — gates a `cu_transquant_bypass_flag`
    /// bit at the very start of **every** `coding_unit()`; a mismatch desyncs
    /// CABAC parsing on the first CU of the first CTU.
    pub transquant_bypass_enabled_flag: bool,
}

impl HevcPps {
    /// Parse a PPS RBSP (2-byte NAL header already stripped).
    ///
    /// # Errors
    ///
    /// [`HevcParamError::Unsupported`] when `tiles_enabled_flag == 1` or
    /// `entropy_coding_sync_enabled_flag == 1` (WPP) — both out of scope, per
    /// the module doc. Other [`HevcParamError::Bitstream`] variants on
    /// truncated/overflowing data.
    pub fn parse(rbsp: &[u8]) -> Result<Self, HevcParamError> {
        let mut reader = BitReader::new(rbsp);
        let pps_pic_parameter_set_id = reader.read_ue()? as u8;
        let pps_seq_parameter_set_id = reader.read_ue()? as u8;
        let dependent_slice_segments_enabled_flag = reader.read_bit()? != 0;
        let output_flag_present_flag = reader.read_bit()? != 0;
        let num_extra_slice_header_bits = reader.read_bits(3)?;
        let sign_data_hiding_enabled_flag = reader.read_bit()? != 0;
        let cabac_init_present_flag = reader.read_bit()? != 0;
        let num_ref_idx_l0_default_active = reader
            .read_ue()?
            .checked_add(1)
            .ok_or(H264Error::FieldOverflow)?;
        let num_ref_idx_l1_default_active = reader
            .read_ue()?
            .checked_add(1)
            .ok_or(H264Error::FieldOverflow)?;
        let init_qp_minus26 = reader.read_se()?;
        let constrained_intra_pred_flag = reader.read_bit()? != 0;
        let transform_skip_enabled_flag = reader.read_bit()? != 0;
        let cu_qp_delta_enabled_flag = reader.read_bit()? != 0;
        let diff_cu_qp_delta_depth = if cu_qp_delta_enabled_flag {
            reader.read_ue()?
        } else {
            0
        };
        let pps_cb_qp_offset = reader.read_se()?;
        let pps_cr_qp_offset = reader.read_se()?;
        let pps_slice_chroma_qp_offsets_present_flag = reader.read_bit()? != 0;
        let weighted_pred_flag = reader.read_bit()? != 0;
        let weighted_bipred_flag = reader.read_bit()? != 0;
        let transquant_bypass_enabled_flag = reader.read_bit()? != 0;
        let tiles_enabled_flag = reader.read_bit()? != 0;
        let entropy_coding_sync_enabled_flag = reader.read_bit()? != 0;
        if tiles_enabled_flag {
            return Err(HevcParamError::Unsupported {
                reason: "tiles_enabled_flag == 1 is not supported",
            });
        }
        if entropy_coding_sync_enabled_flag {
            return Err(HevcParamError::Unsupported {
                reason: "entropy_coding_sync_enabled_flag (WPP) == 1 is not supported",
            });
        }
        // `pps_loop_filter_across_slices_enabled_flag` + deblocking-override
        // fields + scaling-list/extension fields follow — none of these add
        // or remove slice/CTU-level bitstream syntax elements (deblocking
        // parameters only change filter *values*, not bit layout), so this
        // crate's own parser stops here, matching `mediaway_sw::h264::Sps::parse`'s
        // "parse through what changes bit-parsing, not everything" convention.

        let init_qp = init_qp_minus26
            .checked_add(26)
            .ok_or(H264Error::FieldOverflow)?;
        Ok(Self {
            pps_pic_parameter_set_id,
            pps_seq_parameter_set_id,
            output_flag_present_flag,
            num_extra_slice_header_bits,
            num_ref_idx_l0_default_active,
            num_ref_idx_l1_default_active,
            init_qp,
            dependent_slice_segments_enabled_flag,
            sign_data_hiding_enabled_flag,
            cabac_init_present_flag,
            constrained_intra_pred_flag,
            transform_skip_enabled_flag,
            cu_qp_delta_enabled_flag,
            diff_cu_qp_delta_depth,
            pps_cb_qp_offset,
            pps_cr_qp_offset,
            pps_slice_chroma_qp_offsets_present_flag,
            weighted_pred_flag,
            weighted_bipred_flag,
            transquant_bypass_enabled_flag,
        })
    }

    /// Builds the `StdVideoH265PictureParameterSet` this PPS's fields
    /// describe. Every flag/value below is echoed exactly from what this PPS
    /// actually parsed — see [`HevcSps::amp_enabled_flag`]'s doc for the real
    /// hardware bug this crate's HEVC decode hit when several of these were
    /// still hardcoded to `0`/disabled.
    #[must_use]
    pub fn to_std(
        &self,
        sps_video_parameter_set_id: u8,
    ) -> native::StdVideoH265PictureParameterSet {
        let mut flags = native::StdVideoH265PpsFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        };
        flags.set_dependent_slice_segments_enabled_flag(u32::from(
            self.dependent_slice_segments_enabled_flag,
        ));
        flags.set_output_flag_present_flag(u32::from(self.output_flag_present_flag));
        flags.set_sign_data_hiding_enabled_flag(u32::from(self.sign_data_hiding_enabled_flag));
        flags.set_cabac_init_present_flag(u32::from(self.cabac_init_present_flag));
        flags.set_constrained_intra_pred_flag(u32::from(self.constrained_intra_pred_flag));
        flags.set_transform_skip_enabled_flag(u32::from(self.transform_skip_enabled_flag));
        flags.set_cu_qp_delta_enabled_flag(u32::from(self.cu_qp_delta_enabled_flag));
        flags.set_pps_slice_chroma_qp_offsets_present_flag(u32::from(
            self.pps_slice_chroma_qp_offsets_present_flag,
        ));
        flags.set_weighted_pred_flag(u32::from(self.weighted_pred_flag));
        flags.set_weighted_bipred_flag(u32::from(self.weighted_bipred_flag));
        flags.set_transquant_bypass_enabled_flag(u32::from(self.transquant_bypass_enabled_flag));
        native::StdVideoH265PictureParameterSet {
            flags,
            pps_pic_parameter_set_id: self.pps_pic_parameter_set_id,
            pps_seq_parameter_set_id: self.pps_seq_parameter_set_id,
            sps_video_parameter_set_id,
            num_extra_slice_header_bits: self.num_extra_slice_header_bits as u8,
            num_ref_idx_l0_default_active_minus1: (self.num_ref_idx_l0_default_active - 1) as u8,
            num_ref_idx_l1_default_active_minus1: (self.num_ref_idx_l1_default_active - 1) as u8,
            init_qp_minus26: (self.init_qp - 26) as i8,
            diff_cu_qp_delta_depth: self.diff_cu_qp_delta_depth as u8,
            pps_cb_qp_offset: self.pps_cb_qp_offset as i8,
            pps_cr_qp_offset: self.pps_cr_qp_offset as i8,
            pps_beta_offset_div2: 0,
            pps_tc_offset_div2: 0,
            log2_parallel_merge_level_minus2: 0,
            log2_max_transform_skip_block_size_minus2: 0,
            diff_cu_chroma_qp_offset_depth: 0,
            chroma_qp_offset_list_len_minus1: 0,
            cb_qp_offset_list: [0; 6],
            cr_qp_offset_list: [0; 6],
            log2_sao_offset_scale_luma: 0,
            log2_sao_offset_scale_chroma: 0,
            pps_act_y_qp_offset_plus5: 0,
            pps_act_cb_qp_offset_plus5: 0,
            pps_act_cr_qp_offset_plus3: 0,
            pps_num_palette_predictor_initializers: 0,
            luma_bit_depth_entry_minus8: 0,
            chroma_bit_depth_entry_minus8: 0,
            num_tile_columns_minus1: 0,
            num_tile_rows_minus1: 0,
            reserved1: 0,
            reserved2: 0,
            column_width_minus1: [0; 19],
            row_height_minus1: [0; 21],
            reserved3: 0,
            pScalingLists: core::ptr::null(),
            pPredictorPaletteEntries: core::ptr::null(),
        }
    }
}

/// Minimal VPS this crate needs to satisfy
/// `VkVideoDecodeH265SessionParametersAddInfoKHR`.
///
/// This crate does not decode any real VPS syntax element beyond its id
/// (nothing in this crate's own decode path reads VPS fields; the hardware's
/// own bitstream parser does, from the raw NAL bytes it receives directly),
/// mirroring `mediaway-encoder-vulkan::hevc_params::build_vps`'s fixed-shape
/// approach but with a parsed id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HevcVps {
    /// `vps_video_parameter_set_id`.
    pub vps_video_parameter_set_id: u8,
}

impl HevcVps {
    /// Parse a VPS RBSP far enough to extract `vps_video_parameter_set_id`
    /// (the only field this crate's session-parameters construction needs).
    ///
    /// # Errors
    ///
    /// [`HevcParamError::Bitstream`] if `rbsp` is empty.
    pub fn parse(rbsp: &[u8]) -> Result<Self, HevcParamError> {
        let first = *rbsp.first().ok_or(H264Error::UnexpectedEof)?;
        Ok(Self {
            vps_video_parameter_set_id: first >> 4,
        })
    }

    /// Builds the `StdVideoH265VideoParameterSet` this VPS's fields
    /// describe. `profile_tier_level`/`dec_pic_buf_mgr` must outlive the
    /// returned struct (raw pointer fields).
    #[must_use]
    pub fn to_std(
        &self,
        profile_tier_level: &native::StdVideoH265ProfileTierLevel,
        dec_pic_buf_mgr: &native::StdVideoH265DecPicBufMgr,
    ) -> native::StdVideoH265VideoParameterSet {
        let mut flags = native::StdVideoH265VpsFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
            __bindgen_padding_0: [0; 3],
        };
        flags.set_vps_temporal_id_nesting_flag(1);
        native::StdVideoH265VideoParameterSet {
            flags,
            vps_video_parameter_set_id: self.vps_video_parameter_set_id,
            vps_max_sub_layers_minus1: 0,
            reserved1: 0,
            reserved2: 0,
            vps_num_units_in_tick: 0,
            vps_time_scale: 0,
            vps_num_ticks_poc_diff_one_minus1: 0,
            reserved3: 0,
            pDecPicBufMgr: dec_pic_buf_mgr,
            pHrdParameters: core::ptr::null(),
            pProfileTierLevel: profile_tier_level,
        }
    }
}

/// Build one DPB slot's `StdVideoDecodeH265ReferenceInfo` from its
/// [`crate::vulkan::dpb::DpbSlot`] bookkeeping.
///
/// `pic_order_cnt` is reused as `PicOrderCntVal` — HEVC's POC is a single
/// value per picture, unlike H.264's top/bottom pair, matching this crate's
/// progressive-only scope.
#[must_use]
pub fn reference_info_from_slot(
    slot: &crate::vulkan::dpb::DpbSlot,
) -> native::StdVideoDecodeH265ReferenceInfo {
    let mut flags = native::StdVideoDecodeH265ReferenceInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
        __bindgen_padding_0: [0; 3],
    };
    flags.set_used_for_long_term_reference(0);
    flags.set_unused_for_reference(u32::from(!slot.used_for_reference));
    native::StdVideoDecodeH265ReferenceInfo {
        flags,
        PicOrderCntVal: slot.pic_order_cnt,
    }
}

#[cfg(test)]
#[path = "hevc_params_tests.rs"]
mod tests;
