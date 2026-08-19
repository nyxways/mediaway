//! HEVC picture order count (POC) computation (ITU-T H.265 § 8.3.1).
//!
//! Pure, sans-io logic — no D3D12/hardware involved. **Genuinely new logic, no existing
//! port source anywhere in this workspace**: `crate::vulkan::decoder_hevc.rs` only ever
//! reaches "IDR: `PicOrderCntVal` is always `0`" (its own decode path is IDR-only
//! end-to-end, see ADR-0004 § Correcting the task's premise) — no MSB-cycle tracking
//! exists for any HEVC backend before this file. Structurally mirrors `h264_poc.rs`'s
//! `PocState` shape (persistent prev-state struct + `compute()` returning `(poc, Self)`),
//! but HEVC's own formula is a single, simpler case (no H.264-style type-0/1/2 branching)
//! since this module's scope has no field pictures / temporal scalability.

use super::hevc_vps_sps_pps::Sps;

/// Persistent cross-picture POC state (ITU-T H.265 § 8.3.1's `prevTid0Pic`'s own
/// `slice_pic_order_cnt_lsb`/`PicOrderCntMsb`).
///
/// This module's scope has no temporal scalability (`sps_max_sub_layers_minus1 == 0`,
/// enforced by `hevc_vps_sps_pps::parse_sps`), so every accepted picture already has
/// `TemporalId == 0` — "the previous reference picture" and "the previous `TemporalId ==
/// 0` reference picture" (`prevTid0Pic`) are the same thing here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct PocState {
    prev_poc_lsb: u32,
    prev_poc_msb: i32,
}

impl PocState {
    /// Compute `PicOrderCntVal` for the current picture and return the updated state to
    /// persist for the next picture.
    ///
    /// `is_reference` gates the `prev*` state update exactly like H.264's `nal_ref_idc ==
    /// 0` gate (`h264_poc.rs::compute_type0`) — callers must discard the returned state
    /// (keep the old one) for a non-reference picture; this function already applies that
    /// gating internally.
    ///
    /// CRA pictures are rejected upstream (`hevc_vps_sps_pps::HevcNalUnitType::Cra`, see
    /// its own doc for why) — the only picture type that resets `PicOrderCntMsb` to `0`
    /// here is a true IDR (`nal_unit_type` `IDR_W_RADL`/`IDR_N_LP`), which always carries
    /// `pic_order_cnt_lsb == None` (see `hevc_slice::parse_slice_header`).
    #[allow(
        clippy::similar_names,
        reason = "prev_msb/prev_lsb intentionally pair ITU-T H.265 § 8.3.1's own \
        PicOrderCntMsb/pic_order_cnt_lsb naming, mirrors h264_poc.rs::compute_type0's \
        identical allow"
    )]
    pub(super) fn compute(
        self,
        sps: &Sps,
        pic_order_cnt_lsb: Option<u32>,
        is_idr: bool,
        is_reference: bool,
    ) -> (i32, Self) {
        if is_idr {
            let next = if is_reference {
                Self {
                    prev_poc_lsb: 0,
                    prev_poc_msb: 0,
                }
            } else {
                self
            };
            return (0, next);
        }

        let max_poc_lsb = 1i64 << sps.log2_max_pic_order_cnt_lsb;
        let lsb = i64::from(pic_order_cnt_lsb.unwrap_or(0));
        let prev_lsb = i64::from(self.prev_poc_lsb);
        let prev_msb = i64::from(self.prev_poc_msb);
        let msb = if lsb < prev_lsb && (prev_lsb - lsb) >= max_poc_lsb / 2 {
            prev_msb + max_poc_lsb
        } else if lsb > prev_lsb && (lsb - prev_lsb) > max_poc_lsb / 2 {
            prev_msb - max_poc_lsb
        } else {
            prev_msb
        };
        let poc = truncate_i32(msb + lsb);
        let next = if is_reference {
            Self {
                prev_poc_lsb: u32::try_from(lsb).unwrap_or(0),
                prev_poc_msb: truncate_i32(msb),
            }
        } else {
            self
        };
        (poc, next)
    }
}

fn truncate_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

#[cfg(test)]
#[path = "hevc_poc_tests.rs"]
mod tests;
