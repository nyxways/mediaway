//! H.264 picture order count (POC) computation, types 0/1/2 (ITU-T H.264 § 8.2.1).
//!
//! Pure, sans-io logic — no D3D12/hardware involved. This is exactly the kind of
//! bookkeeping ADR-0002 calls out as new relative to every other backend in this
//! workspace (WMF/VA-API/D3D12-encode are all IDR-only, so none of them need POC at
//! all).

use super::h264_slice::SliceHeader;
use super::h264_sps_pps::Sps;

/// Persistent cross-picture POC state (ITU-T H.264 § 8.2.1's `prevPicOrderCntMsb`/
/// `prevPicOrderCntLsb` for type 0, `prevFrameNum`/`prevFrameNumOffset` for types 1/2).
///
/// No `memory_management_control_operation == 5` support (this stage rejects adaptive
/// marking entirely, see `h264_slice.rs`), so the "previous reference picture had MMCO 5"
/// special case in § 8.2.1.1 never applies here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_field_names,
    reason = "the shared `prev_` prefix mirrors ITU-T H.264 § 8.2.1's own \
    prevPicOrderCntMsb/prevPicOrderCntLsb/prevFrameNum/prevFrameNumOffset naming"
)]
pub(super) struct PocState {
    prev_pic_order_cnt_msb: i32,
    prev_pic_order_cnt_lsb: i32,
    prev_frame_num: u32,
    prev_frame_num_offset: i32,
}

/// One picture's computed POC (frame pictures only — this crate's SPS parser already
/// rejects field/MBAFF coding, so `TopFieldOrderCnt == BottomFieldOrderCnt` is not
/// generally true, but there is only ever one frame's worth of POC to track per picture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_field_names,
    reason = "the shared `_order_cnt` suffix mirrors ITU-T H.264 § 8.2's own \
    PicOrderCnt/TopFieldOrderCnt/BottomFieldOrderCnt naming"
)]
pub(super) struct Poc {
    /// `PicOrderCnt` used for reference-list ordering (§ 8.2.4): `min(Top, Bottom)`.
    pub(super) pic_order_cnt: i32,
    pub(super) top_field_order_cnt: i32,
    pub(super) bottom_field_order_cnt: i32,
}

impl PocState {
    /// Compute POC for the current picture and return the updated state to persist
    /// for the *next* picture.
    ///
    /// Real H.264 only updates `prev*` state from reference pictures — callers must
    /// discard the returned state (keep the old one) when `nal_ref_idc == 0`; this
    /// function already applies that gating internally per § 8.2.1.
    pub(super) fn compute(
        self,
        sps: &Sps,
        sh: &SliceHeader,
        is_idr: bool,
        nal_ref_idc: u8,
    ) -> (Poc, Self) {
        match sps.pic_order_cnt_type {
            1 => self.compute_type1(sps, sh, is_idr, nal_ref_idc),
            2 => self.compute_type2(sps, sh, is_idr, nal_ref_idc),
            _ => self.compute_type0(sps, sh, is_idr, nal_ref_idc),
        }
    }

    #[allow(
        clippy::similar_names,
        reason = "prev_msb/prev_lsb intentionally pair § 8.2.1.1's own prevPicOrderCntMsb/prevPicOrderCntLsb naming"
    )]
    fn compute_type0(
        self,
        sps: &Sps,
        sh: &SliceHeader,
        is_idr: bool,
        nal_ref_idc: u8,
    ) -> (Poc, Self) {
        let max_poc_lsb = 1i64 << sps.log2_max_pic_order_cnt_lsb;
        let (prev_msb, prev_lsb) = if is_idr {
            (0i64, 0i64)
        } else {
            (
                i64::from(self.prev_pic_order_cnt_msb),
                i64::from(self.prev_pic_order_cnt_lsb),
            )
        };
        let lsb = i64::from(sh.pic_order_cnt_lsb);
        let msb = if lsb < prev_lsb && (prev_lsb - lsb) >= max_poc_lsb / 2 {
            prev_msb + max_poc_lsb
        } else if lsb > prev_lsb && (lsb - prev_lsb) > max_poc_lsb / 2 {
            prev_msb - max_poc_lsb
        } else {
            prev_msb
        };
        let top = msb + lsb;
        let bottom = top + i64::from(sh.delta_pic_order_cnt_bottom);
        let poc = Poc {
            pic_order_cnt: truncate_i32(top.min(bottom)),
            top_field_order_cnt: truncate_i32(top),
            bottom_field_order_cnt: truncate_i32(bottom),
        };
        let next = if nal_ref_idc == 0 {
            self
        } else {
            Self {
                prev_pic_order_cnt_msb: truncate_i32(msb),
                prev_pic_order_cnt_lsb: truncate_i32(lsb),
                ..self
            }
        };
        (poc, next)
    }

    fn compute_type1(
        self,
        sps: &Sps,
        sh: &SliceHeader,
        is_idr: bool,
        nal_ref_idc: u8,
    ) -> (Poc, Self) {
        let max_frame_num = 1i64 << sps.log2_max_frame_num;
        let frame_num = i64::from(sh.frame_num);
        let frame_num_offset = if is_idr {
            0i64
        } else if i64::from(self.prev_frame_num) > frame_num {
            i64::from(self.prev_frame_num_offset) + max_frame_num
        } else {
            i64::from(self.prev_frame_num_offset)
        };

        let cycle_len = i64::try_from(sps.offset_for_ref_frame.len()).unwrap_or(0);
        let abs_frame_num_raw = if cycle_len != 0 {
            frame_num_offset + frame_num
        } else {
            0
        };
        let abs_frame_num = if nal_ref_idc == 0 && abs_frame_num_raw > 0 {
            abs_frame_num_raw - 1
        } else {
            abs_frame_num_raw
        };

        let expected_delta_per_cycle: i64 =
            sps.offset_for_ref_frame.iter().map(|&v| i64::from(v)).sum();
        let mut expected_poc = 0i64;
        if abs_frame_num > 0 && cycle_len != 0 {
            let cycle_cnt = (abs_frame_num - 1) / cycle_len;
            let in_cycle = usize::try_from((abs_frame_num - 1) % cycle_len).unwrap_or(0);
            expected_poc = cycle_cnt * expected_delta_per_cycle;
            for offset in &sps.offset_for_ref_frame[..=in_cycle] {
                expected_poc += i64::from(*offset);
            }
        }
        if nal_ref_idc == 0 {
            expected_poc += i64::from(sps.offset_for_non_ref_pic);
        }

        let top = expected_poc + i64::from(sh.delta_pic_order_cnt[0]);
        let bottom = top
            + i64::from(sps.offset_for_top_to_bottom_field)
            + i64::from(sh.delta_pic_order_cnt[1]);
        let poc = Poc {
            pic_order_cnt: truncate_i32(top.min(bottom)),
            top_field_order_cnt: truncate_i32(top),
            bottom_field_order_cnt: truncate_i32(bottom),
        };
        let next = Self {
            prev_frame_num: sh.frame_num,
            prev_frame_num_offset: truncate_i32(frame_num_offset),
            ..self
        };
        (poc, next)
    }

    fn compute_type2(
        self,
        sps: &Sps,
        sh: &SliceHeader,
        is_idr: bool,
        nal_ref_idc: u8,
    ) -> (Poc, Self) {
        let max_frame_num = 1i64 << sps.log2_max_frame_num;
        let frame_num = i64::from(sh.frame_num);
        let frame_num_offset = if is_idr {
            0i64
        } else if i64::from(self.prev_frame_num) > frame_num {
            i64::from(self.prev_frame_num_offset) + max_frame_num
        } else {
            i64::from(self.prev_frame_num_offset)
        };
        let temp_poc = if is_idr {
            0i64
        } else if nal_ref_idc == 0 {
            2 * (frame_num_offset + frame_num) - 1
        } else {
            2 * (frame_num_offset + frame_num)
        };
        let temp_poc = truncate_i32(temp_poc);
        let poc = Poc {
            pic_order_cnt: temp_poc,
            top_field_order_cnt: temp_poc,
            bottom_field_order_cnt: temp_poc,
        };
        let next = Self {
            prev_frame_num: sh.frame_num,
            prev_frame_num_offset: truncate_i32(frame_num_offset),
            ..self
        };
        (poc, next)
    }
}

fn truncate_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

#[cfg(test)]
#[path = "h264_poc_tests.rs"]
mod tests;
