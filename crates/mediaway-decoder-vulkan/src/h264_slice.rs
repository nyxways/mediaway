//! H.264 slice header parsing (I/P slices) and `RefPicList0` construction.
//!
//! Not reused from `mediaway_sw::h264::SliceHeader` — see `h264_params.rs`'s
//! module doc and `adr/0001`'s "Bitstream-parser reuse" section: that type
//! only parses I-slices and never builds a reference picture list.
//!
//! **Important finding (not assumed by `adr/0001`, confirmed against real
//! `vulkanalia` 0.35 bindings)**: `StdVideoDecodeH264PictureInfo` has no
//! `pRefLists`-equivalent field, and no `StdVideoDecodeH264ReferenceListsInfo`
//! struct exists in this crate's Vulkan bindings at all (unlike the H.264
//! **encode** side, whose `StdVideoEncodeH264PictureInfo` does carry
//! `pRefLists`). For decode, the hardware itself parses `ref_pic_list_
//! modification()`/`dec_ref_pic_marking()` directly from the raw slice NAL
//! bytes handed to `vkCmdDecodeVideoKHR` (`src_buffer`/`slice_offsets`) — the
//! application only supplies the *set* of currently-valid DPB reference
//! slots (`StdVideoDecodeH264ReferenceInfo` per slot, via
//! `VkVideoDecodeInfoKHR::reference_slots`), not an explicit ordered list.
//! [`default_ref_pic_list0`]/[`apply_ref_pic_list_modifications`] below are
//! therefore not required to feed any Vulkan call this crate makes — they
//! exist as sans-io, spec-accurate, independently testable documentation of
//! what the hardware's own list construction is expected to produce (ITU-T
//! H.264 § 8.2.4.2.1 / § 8.2.4.3.1), useful for validating DPB bookkeeping
//! and for a future software-fallback decoder that would need a real list.
//!
//! Scope cut this round: **B-slices are rejected** (`slice_type == B` ->
//! [`H264ParamError::Unsupported`]) — this round's DPB/reference-management
//! work targets P-slice `RefPicList0` only; `RefPicList1`/B-slice support is
//! explicit follow-up work, not silently assumed solved. Matches this
//! workspace's D3D12 sibling ADR's own scope cut for consistency across the
//! two decode backends. Only sliding-window `dec_ref_pic_marking` is
//! supported — `adaptive_ref_pic_marking_mode_flag == 1` is rejected as
//! [`H264ParamError::Unsupported`] (per `adr/0001`'s scope).

#![forbid(unsafe_code)]

use mediaway_sw::h264::{BitReader, H264Error, NalUnitType};
use smallvec::SmallVec;

use crate::dpb::Dpb;
use crate::h264_params::{H264ParamError, H264Pps, H264Sps};

/// Base `slice_type` (ITU-T H.264 Table 7-6).
///
/// `slice_type % 5` already normalized (a real bitstream may add 5 to signal
/// "every slice in this picture shares this type" — normalized away here the
/// same way `mediaway_sw::h264::SliceType` does).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum H264SliceType {
    /// Predictive slice — `RefPicList0` only, this crate's supported inter
    /// slice type this round.
    P,
    /// Bi-predictive slice — not decoded this round (see module doc).
    B,
    /// Intra-only slice.
    I,
    /// Switching P slice — rare, streaming-specific, not decoded this round.
    Sp,
    /// Switching I slice — rare, streaming-specific, not decoded this round.
    Si,
}

impl H264SliceType {
    #[must_use]
    const fn from_raw(slice_type: u32) -> Self {
        match slice_type % 5 {
            0 => Self::P,
            1 => Self::B,
            3 => Self::Sp,
            4 => Self::Si,
            _ => Self::I,
        }
    }
}

/// One `ref_pic_list_modification()` operation (ITU-T H.264 § 7.3.3.1).
///
/// Retained (not just skipped) so [`apply_ref_pic_list_modifications`] can
/// use it — see the module doc for why the hardware does not need this from
/// the application directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefPicListModification {
    /// `modification_of_pic_nums_idc` (`0` or `1`: short-term, subtract/add;
    /// `2`: long-term, not supported this round — see [`H264SliceHeader::parse`]).
    pub idc: u32,
    /// `abs_diff_pic_num_minus1` (when `idc` is `0`/`1`).
    pub value: u32,
}

/// Parsed H.264 slice header fields this crate's decode session needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H264SliceHeader {
    /// `first_mb_in_slice` — must be `0` (single slice per picture, matching
    /// this crate's scope cut on multi-slice generality).
    pub first_mb_in_slice: u32,
    /// Decoded, normalized slice type.
    pub slice_type: H264SliceType,
    /// `pic_parameter_set_id`.
    pub pic_parameter_set_id: u32,
    /// `frame_num`.
    pub frame_num: u32,
    /// `idr_pic_id`, present only for IDR slices.
    pub idr_pic_id: Option<u32>,
    /// `pic_order_cnt_lsb` (always present: this crate requires
    /// `pic_order_cnt_type == 0`, see `h264_params.rs`).
    pub pic_order_cnt_lsb: u32,
    /// Active `RefPicList0` size: `num_ref_idx_l0_active_minus1 + 1` when
    /// overridden, else `pps.num_ref_idx_l0_default_active`. `0` for I slices.
    pub num_ref_idx_l0_active: u32,
    /// `ref_pic_list_modification_flag_l0` and its operations, present only
    /// for P slices.
    pub ref_pic_list_modifications_l0: SmallVec<[RefPicListModification; 4]>,
    /// `slice_qp_delta`.
    pub slice_qp_delta: i32,
}

impl H264SliceHeader {
    /// Parse a slice header from `reader`, positioned at the first bit after
    /// the NAL header byte (RBSP already de-emulated, e.g. via
    /// [`mediaway_sw::h264::NalUnit::parse`]).
    ///
    /// # Errors
    ///
    /// [`H264ParamError::Unsupported`] for B/SP/SI slices, field pictures,
    /// `first_mb_in_slice != 0`, or `adaptive_ref_pic_marking_mode_flag == 1`.
    /// Other [`H264ParamError::Bitstream`] variants on truncated data.
    pub fn parse(
        reader: &mut BitReader<'_>,
        sps: &H264Sps,
        pps: &H264Pps,
        nal_unit_type: NalUnitType,
        nal_ref_idc: u8,
    ) -> Result<Self, H264ParamError> {
        let first_mb_in_slice = reader.read_ue()?;
        if first_mb_in_slice != 0 {
            return Err(H264ParamError::Unsupported {
                reason: "multi-slice pictures are not supported (first_mb_in_slice must be 0)",
            });
        }
        let slice_type = H264SliceType::from_raw(reader.read_ue()?);
        if matches!(
            slice_type,
            H264SliceType::B | H264SliceType::Sp | H264SliceType::Si
        ) {
            return Err(H264ParamError::Unsupported {
                reason: "only I- and P-slices are supported this round",
            });
        }
        let pic_parameter_set_id = reader.read_ue()?;
        let frame_num = reader.read_bits(sps.log2_max_frame_num)?;
        // field_pic_flag is only present when frame_mbs_only_flag == 0, which
        // h264_params.rs already rejects at the SPS level — nothing to read
        // here.

        let is_idr = matches!(nal_unit_type, NalUnitType::IdrSlice);
        let idr_pic_id = if is_idr {
            Some(reader.read_ue()?)
        } else {
            None
        };

        let pic_order_cnt_lsb = reader.read_bits(sps.log2_max_pic_order_cnt_lsb)?;
        // `redundant_pic_cnt` is only present when
        // `pps.redundant_pic_cnt_present_flag` — `h264_params.rs`'s `H264Pps`
        // does not retain that flag (assumed `0`, the overwhelmingly common
        // case, matching `mediaway_sw::h264::SliceHeader`'s identical note).

        let mut num_ref_idx_l0_active = 0u32;
        let mut ref_pic_list_modifications_l0 = SmallVec::new();
        if matches!(slice_type, H264SliceType::P) {
            let num_ref_idx_active_override_flag = reader.read_bit()? != 0;
            num_ref_idx_l0_active = if num_ref_idx_active_override_flag {
                reader
                    .read_ue()?
                    .checked_add(1)
                    .ok_or(H264Error::FieldOverflow)?
            } else {
                pps.num_ref_idx_l0_default_active
            };

            let ref_pic_list_modification_flag_l0 = reader.read_bit()? != 0;
            if ref_pic_list_modification_flag_l0 {
                loop {
                    let idc = reader.read_ue()?;
                    if idc == 3 {
                        break;
                    }
                    if idc == 2 {
                        return Err(H264ParamError::Unsupported {
                            reason: "long-term reference picture modification is not supported",
                        });
                    }
                    let value = reader.read_ue()?;
                    ref_pic_list_modifications_l0.push(RefPicListModification { idc, value });
                }
            }
        }

        if nal_ref_idc != 0 {
            parse_dec_ref_pic_marking(reader, is_idr)?;
        }
        // `cabac_init_idc` only applies to CABAC-entropy-coded P/SP/B slices;
        // this crate rejects CABAC entirely (`h264_params.rs`'s SPS/PPS
        // parsing has no CABAC gate today, but every real stream this crate
        // targets uses CAVLC — see `adr/0001`) so it is never read here.

        let slice_qp_delta = reader.read_se()?;

        // `deblocking_filter_control_present_flag`-gated fields (ITU-T H.264
        // § 7.3.3) — present only when the PPS signals it. Values are not
        // retained: the hardware decoder parses its own copy directly from
        // the raw bitstream bytes fed to `vkCmdDecodeVideoKHR` (see the
        // module doc); only correct bit-position advancement (so nothing
        // downstream misparses) matters here.
        if pps.deblocking_filter_control_present {
            let disable_deblocking_filter_idc = reader.read_ue()?;
            if disable_deblocking_filter_idc != 1 {
                let _slice_alpha_c0_offset_div2 = reader.read_se()?;
                let _slice_beta_offset_div2 = reader.read_se()?;
            }
        }

        Ok(Self {
            first_mb_in_slice,
            slice_type,
            pic_parameter_set_id,
            frame_num,
            idr_pic_id,
            pic_order_cnt_lsb,
            num_ref_idx_l0_active,
            ref_pic_list_modifications_l0,
            slice_qp_delta,
        })
    }
}

/// Parse `dec_ref_pic_marking()` (ITU-T H.264 § 7.3.3.3). Only sliding-window
/// marking is supported — `adaptive_ref_pic_marking_mode_flag == 1` streams
/// are rejected (see the module doc's scope cut). Values are not retained:
/// this crate's own [`Dpb::allocate_slot`](crate::dpb::Dpb::allocate_slot)
/// implements the sliding-window process directly from `frame_num_wrap`, so
/// only correct bit-position advancement (and detecting the unsupported
/// adaptive case) matters here.
fn parse_dec_ref_pic_marking(
    reader: &mut BitReader<'_>,
    is_idr: bool,
) -> Result<(), H264ParamError> {
    if is_idr {
        let _no_output_of_prior_pics_flag = reader.read_bit()?;
        let _long_term_reference_flag = reader.read_bit()?;
        return Ok(());
    }
    let adaptive_ref_pic_marking_mode_flag = reader.read_bit()? != 0;
    if adaptive_ref_pic_marking_mode_flag {
        return Err(H264ParamError::Unsupported {
            reason: "adaptive_ref_pic_marking_mode_flag (MMCO) is not supported; sliding window only",
        });
    }
    Ok(())
}

/// Default `RefPicList0` initialization for a P slice (ITU-T H.264 § 8.2.4.2.1,
/// short-term-only case).
///
/// Every occupied reference slot, sorted by decreasing `frame_num_wrap`
/// (`PicNum`, since this crate does not support long-term references). See
/// the module doc for why this is not fed to any Vulkan call — it exists for
/// sans-io validation/testing.
#[must_use]
pub fn default_ref_pic_list0(dpb: &Dpb) -> Vec<usize> {
    let mut refs: Vec<(usize, i32)> = dpb
        .occupied_slots()
        .filter(|(_, slot)| slot.used_for_reference)
        .map(|(index, slot)| (index, slot.frame_num_wrap))
        .collect();
    refs.sort_by_key(|&(_, frame_num_wrap)| std::cmp::Reverse(frame_num_wrap));
    refs.into_iter().map(|(index, _)| index).collect()
}

/// Apply `ref_pic_list_modification()` operations to a default `RefPicList0`
/// (ITU-T H.264 § 8.2.4.3.1, short-term-only case — `idc` values `0`/`1`).
///
/// `current_pic_num`/`max_pic_num` are the current picture's `frame_num` and
/// `MaxFrameNum` respectively (equal to `PicNum`/`MaxPicNum` for frame
/// pictures, since field pictures are out of scope).
#[must_use]
pub fn apply_ref_pic_list_modifications(
    mut ref_pic_list0: Vec<usize>,
    dpb: &Dpb,
    modifications: &[RefPicListModification],
    current_pic_num: i32,
    max_pic_num: i32,
) -> Vec<usize> {
    let mut pred_pic_num = current_pic_num;
    let mut insert_at = 0usize;
    for modification in modifications {
        if modification.idc != 0 && modification.idc != 1 {
            continue;
        }
        let abs_diff = i32::try_from(modification.value.wrapping_add(1)).unwrap_or(i32::MAX);
        let pic_num_no_wrap = if modification.idc == 0 {
            let value = pred_pic_num - abs_diff;
            if value < 0 {
                value + max_pic_num
            } else {
                value
            }
        } else {
            let value = pred_pic_num + abs_diff;
            if value >= max_pic_num {
                value - max_pic_num
            } else {
                value
            }
        };
        pred_pic_num = pic_num_no_wrap;
        let pic_num = if pic_num_no_wrap > current_pic_num {
            pic_num_no_wrap - max_pic_num
        } else {
            pic_num_no_wrap
        };

        let target = dpb
            .occupied_slots()
            .find(|(_, slot)| slot.used_for_reference && slot.frame_num_wrap == pic_num)
            .map(|(index, _)| index);
        if let Some(target_index) = target {
            ref_pic_list0.retain(|&index| index != target_index);
            let at = insert_at.min(ref_pic_list0.len());
            ref_pic_list0.insert(at, target_index);
            insert_at += 1;
        }
    }
    ref_pic_list0
}

#[cfg(test)]
#[path = "h264_slice_tests.rs"]
mod tests;
