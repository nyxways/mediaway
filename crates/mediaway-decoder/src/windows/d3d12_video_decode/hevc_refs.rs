//! HEVC DPB "RPS application" eviction (ITU-T H.265 § 8.3.2) and
//! `RefPicList`/`RefPicSetStCurrBefore`/`RefPicSetStCurrAfter` construction for
//! `DXVA_PicParams_HEVC`. Pure, sans-io logic — no D3D12/hardware involved.
//!
//! **No port available** — `crate::vulkan::decoder_hevc.rs` never reaches this problem
//! (its own decode path is IDR-only end-to-end, so it never builds a real reference list),
//! per ADR-0004 § File layout plan. Structurally different from `h264_refs.rs`'s
//! `FrameNumWrap` sliding window: HEVC's DPB eviction is driven by whether a slot's POC
//! appears anywhere in the current picture's own signaled RPS, not a frame-count window.
//!
//! **`RefPicSetStCurrBefore`/`After` index semantics — not independently confirmed from a
//! primary source** (ADR-0004 § `RefPicList`/... semantics, its own Open Question #3):
//! believed to be byte-indices into `RefPicList[15]` (`0..15`, `0xFF` unused), not raw DPB
//! slot numbers, matching every other DXVA struct's `used_for_reference_flags`-style
//! indirection in this family. **First thing to confirm against `libavcodec/dxva2_hevc.c`
//! before any real hardware attempt** — this module implements the *believed* semantics,
//! not a hardware-verified one.

use crate::DecodeError;
use smallvec::SmallVec;

/// Per-reference metadata this module needs to place a DPB slot in a reference list.
/// Stored by [`super::dpb::DpbPool`] alongside each occupied, `is_reference` slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HevcRefMeta {
    /// `PicOrderCntVal` (ITU-T H.265 § 8.3.1) — HEVC has one POC per picture (no
    /// top/bottom pair, unlike H.264's `H264RefMeta`), matching this module's
    /// progressive-only scope.
    pub(super) poc: i32,
}

/// Which currently-held DPB references (`refs`) the RPS "application" process (ITU-T
/// H.265 § 8.3.2) must evict before the current picture can be decoded: any reference
/// whose POC is not named anywhere in `all_rps_poc` (both `used_by_curr_pic` and "foll"
/// entries — see `hevc_slice::ShortTermRefPicSet::all_poc`'s doc for why the full set,
/// not just the "used by current picture" subset, is the right input here).
///
/// Returns DPB slot indices, not references — [`super::dpb::SlotTable::evict`] is the
/// caller's job (this function has no D3D12/DPB-pool dependency, kept unit-testable).
pub(super) fn slots_to_evict(
    refs: &[(u32, HevcRefMeta)],
    all_rps_poc: &[i32],
) -> SmallVec<[u32; 16]> {
    refs.iter()
        .filter(|&&(_, meta)| !all_rps_poc.contains(&meta.poc))
        .map(|&(slot, _)| slot)
        .collect()
}

/// Constructed `RefPicList[15]`/`PicOrderCntValList[15]`/`RefPicSetStCurrBefore[8]`/
/// `RefPicSetStCurrAfter[8]` inputs for [`super::hevc_pic_params::build_pic_params`] —
/// DPB-slot-agnostic (holds slot indices as plain `u32`, no `ID3D12Resource`).
#[derive(Debug, Clone, Default)]
pub(super) struct HevcRefLists {
    /// One entry per currently-active DPB reference (up to 15), in `refs`' own order.
    pub(super) ref_pic_list: SmallVec<[u32; 15]>,
    /// `PicOrderCntValList[15]` — parallel to `ref_pic_list`.
    pub(super) poc_list: SmallVec<[i32; 15]>,
    /// Byte-indices into `ref_pic_list` (see module doc's semantics caveat) naming which
    /// entries are `RefPicSetStCurrBefore` — this module's single-forward-reference scope
    /// (`hevc_slice.rs`'s own `num_curr_pics() == 1` check) means at most one of
    /// `st_curr_before`/`st_curr_after` ever holds a real entry.
    pub(super) st_curr_before: SmallVec<[u8; 8]>,
    pub(super) st_curr_after: SmallVec<[u8; 8]>,
}

/// Build [`HevcRefLists`] from the DPB's active references (`refs`, **after** this
/// picture's own eviction pass — see [`slots_to_evict`]) and the current picture's
/// `RefPicSetStCurrBefore`/`After` POC values (`hevc_slice::ShortTermRefPicSet::
/// curr_before_after_poc`'s output).
///
/// # Errors
///
/// [`DecodeError::InvalidInput`] when a `before_poc`/`after_poc` value has no matching
/// entry in `refs` — an internally-inconsistent stream (the picture it names is not
/// actually held in the DPB).
pub(super) fn build_ref_lists(
    refs: &[(u32, HevcRefMeta)],
    before_poc: &[i32],
    after_poc: &[i32],
) -> Result<HevcRefLists, DecodeError> {
    let mut out = HevcRefLists::default();
    for &(slot, meta) in refs.iter().take(15) {
        out.ref_pic_list.push(slot);
        out.poc_list.push(meta.poc);
    }

    let find_index = |poc: i32| -> Result<u8, DecodeError> {
        out.poc_list
            .iter()
            .position(|&p| p == poc)
            .and_then(|i| u8::try_from(i).ok())
            .ok_or(DecodeError::InvalidInput)
    };
    for &poc in before_poc.iter().take(8) {
        out.st_curr_before.push(find_index(poc)?);
    }
    for &poc in after_poc.iter().take(8) {
        out.st_curr_after.push(find_index(poc)?);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "hevc_refs_tests.rs"]
mod tests;
