//! H.264 `RefPicList0`/`RefPicList1` construction (ITU-T H.264 § 8.2.4) and DPB
//! sliding-window eviction (§ 8.2.5.3). Pure, sans-io logic — no D3D12/hardware
//! involved; generic over nothing D3D12-specific so [`super::dpb::DpbPool`] can stay
//! generic over per-codec reference metadata too.
//!
//! **Scope this stage** (ADR-0002): sliding-window marking only — no MMCO/adaptive
//! marking, no long-term references. `h264_slice.rs` already rejects
//! `adaptive_ref_pic_marking_mode_flag` and long-term `ref_pic_list_modification`
//! operations before a slice reaches this module.

use mediaway_decoder::DecodeError;

use super::h264_slice::RefPicListModOp;

/// Per-reference metadata this module needs to place a DPB slot in a reference list.
/// Stored by [`super::dpb::DpbPool`] alongside each occupied, `is_reference` slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct H264RefMeta {
    pub(super) frame_num: u32,
    /// `PicOrderCnt` (§ 8.2.1's `min(TopFieldOrderCnt, BottomFieldOrderCnt)`) — used
    /// for B-slice reference-list ordering.
    pub(super) poc: i32,
    /// Retained (alongside the derived `poc` above) because `h264_pic_params` packs
    /// both fields into `DXVA_PicParams_H264::FieldOrderCntList` verbatim, not just
    /// the derived minimum.
    pub(super) top_field_order_cnt: i32,
    pub(super) bottom_field_order_cnt: i32,
}

/// One entry in a constructed `RefPicListX`: the DPB slot index paired with the
/// values used to order/select it (`PicNum` for P-slice ordering and
/// `ref_pic_list_modification`, `PicOrderCnt` for B-slice ordering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RefListEntry {
    pub(super) slot: u32,
    pub(super) pic_num: i32,
    pub(super) poc: i32,
}

/// `FrameNumWrap` (§ 8.2.4.1) relative to the picture currently being decoded.
fn frame_num_wrap(ref_frame_num: u32, curr_frame_num: u32, max_frame_num: u32) -> i32 {
    let (ref_fn, curr_fn, max_fn) = (
        i64::from(ref_frame_num),
        i64::from(curr_frame_num),
        i64::from(max_frame_num),
    );
    let wrap = if ref_fn > curr_fn {
        ref_fn - max_fn
    } else {
        ref_fn
    };
    i32::try_from(wrap).unwrap_or(i32::MIN)
}

/// `PicNum` for a frame picture (§ 8.2.4.1: `PicNum == FrameNumWrap` when
/// `field_pic_flag == 0`, which `h264_sps_pps`/`h264_slice` already require).
fn pic_num(meta: H264RefMeta, curr_frame_num: u32, max_frame_num: u32) -> i32 {
    frame_num_wrap(meta.frame_num, curr_frame_num, max_frame_num)
}

fn to_entries(
    refs: &[(u32, H264RefMeta)],
    curr_frame_num: u32,
    max_frame_num: u32,
) -> Vec<RefListEntry> {
    refs.iter()
        .map(|&(slot, meta)| RefListEntry {
            slot,
            pic_num: pic_num(meta, curr_frame_num, max_frame_num),
            poc: meta.poc,
        })
        .collect()
}

/// Default `RefPicList0` construction for a P/SP slice (§ 8.2.4.2.1): descending
/// `PicNum`.
pub(super) fn build_default_list_p(
    refs: &[(u32, H264RefMeta)],
    curr_frame_num: u32,
    max_frame_num: u32,
) -> Vec<RefListEntry> {
    let mut list = to_entries(refs, curr_frame_num, max_frame_num);
    list.sort_by_key(|e| std::cmp::Reverse(e.pic_num));
    list
}

/// Default `RefPicList0`/`RefPicList1` construction for a B slice (§ 8.2.4.2.3):
/// split by `PicOrderCnt` relative to the current picture, then interleave; swap the
/// first two `RefPicList1` entries when it would otherwise be identical to
/// `RefPicList0`.
pub(super) fn build_default_lists_b(
    refs: &[(u32, H264RefMeta)],
    curr_frame_num: u32,
    max_frame_num: u32,
    curr_poc: i32,
) -> (Vec<RefListEntry>, Vec<RefListEntry>) {
    let entries = to_entries(refs, curr_frame_num, max_frame_num);

    let mut before: Vec<RefListEntry> = entries
        .iter()
        .copied()
        .filter(|e| e.poc < curr_poc)
        .collect();
    before.sort_by_key(|e| std::cmp::Reverse(e.poc));
    let mut after: Vec<RefListEntry> = entries
        .iter()
        .copied()
        .filter(|e| e.poc > curr_poc)
        .collect();
    after.sort_by_key(|a| a.poc);

    let mut list0 = before.clone(); // clone: `before` also seeds RefPicList1's tail below
    list0.extend(after.iter().copied());

    let mut list1 = after.clone(); // clone: `after` also seeded RefPicList0's head above
    list1.extend(before.iter().copied());

    if list1.len() > 1 && list1 == list0 {
        list1.swap(0, 1);
    }
    (list0, list1)
}

/// Pad a constructed list to `len` entries by repeating its last entry (defensive —
/// real encoders always provide enough references, but a malformed/short DPB must not
/// panic on out-of-range access downstream). Returns [`DecodeError::InvalidInput`]
/// when `len > 0` and the list has no entries to repeat.
fn pad_to_length(list: &mut Vec<RefListEntry>, len: usize) -> Result<(), DecodeError> {
    let Some(&tail_entry) = list.last() else {
        return if len == 0 {
            Ok(())
        } else {
            Err(DecodeError::InvalidInput)
        };
    };
    while list.len() < len {
        list.push(tail_entry);
    }
    Ok(())
}

/// Apply `ref_pic_list_modification()` (§ 8.2.4.3.1) in place, then pad/truncate to
/// `num_ref_idx_active` entries.
///
/// # Errors
///
/// [`DecodeError::InvalidInput`] when a modification op references a `PicNum` not
/// present in `list` (an internally-inconsistent or malformed stream — the picture it
/// names is not actually held in the DPB).
pub(super) fn apply_modifications(
    list: &mut Vec<RefListEntry>,
    ops: &[RefPicListModOp],
    curr_frame_num: u32,
    max_frame_num: u32,
    num_ref_idx_active: usize,
) -> Result<(), DecodeError> {
    let max_pic_num = i64::from(max_frame_num);
    let mut pred = i64::from(curr_frame_num);
    let mut idx = 0usize;
    for op in ops {
        let diff = i64::from(op.abs_diff_pic_num_minus1) + 1;
        let no_wrap = if op.add {
            let v = pred + diff;
            if v >= max_pic_num { v - max_pic_num } else { v }
        } else {
            let v = pred - diff;
            if v < 0 { v + max_pic_num } else { v }
        };
        pred = no_wrap;
        let pic_num_lx = if no_wrap > i64::from(curr_frame_num) {
            no_wrap - max_pic_num
        } else {
            no_wrap
        };
        let pic_num_lx = i32::try_from(pic_num_lx).unwrap_or(i32::MIN);

        let found_pos = list
            .iter()
            .skip(idx)
            .position(|e| e.pic_num == pic_num_lx)
            .map(|p| p + idx);
        let Some(found_pos) = found_pos else {
            return Err(DecodeError::InvalidInput);
        };
        let entry = list.remove(found_pos);
        list.insert(idx, entry);
        idx += 1;
        if let Some(dup_pos) = list.iter().skip(idx).position(|e| e.slot == entry.slot) {
            list.remove(idx + dup_pos);
        }
    }
    pad_to_length(list, num_ref_idx_active)?;
    list.truncate(num_ref_idx_active);
    Ok(())
}

/// Decide which currently-held short-term reference (if any) the sliding-window
/// process (§ 8.2.5.3) must evict before the current picture can be marked as a new
/// reference, given `max_num_ref_frames` short-term references are already held.
///
/// Returns `None` when there is still room (no eviction needed this picture).
pub(super) fn sliding_window_evict(
    refs: &[(u32, H264RefMeta)],
    curr_frame_num: u32,
    max_frame_num: u32,
    max_num_ref_frames: u32,
) -> Option<u32> {
    if refs.is_empty() || refs.len() < max_num_ref_frames.max(1) as usize {
        return None;
    }
    refs.iter()
        .min_by_key(|&&(_, meta)| frame_num_wrap(meta.frame_num, curr_frame_num, max_frame_num))
        .map(|&(slot, _)| slot)
}

#[cfg(test)]
#[path = "h264_refs_tests.rs"]
mod tests;
