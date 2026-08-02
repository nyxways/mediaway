//! Pure unit tests for [`super`]'s `RefPicList0`/`RefPicList1` construction and
//! sliding-window DPB eviction — hand-built fixtures, no D3D12/hardware involved.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{
    H264RefMeta, RefListEntry, apply_modifications, build_default_list_p, build_default_lists_b,
    pad_to_length, sliding_window_evict,
};
use crate::d3d12_video_decode::h264_slice::RefPicListModOp;

fn meta(frame_num: u32, poc: i32) -> H264RefMeta {
    H264RefMeta {
        frame_num,
        poc,
        top_field_order_cnt: poc,
        bottom_field_order_cnt: poc,
    }
}

fn slots(entries: &[RefListEntry]) -> Vec<u32> {
    entries.iter().map(|e| e.slot).collect()
}

#[test]
fn default_list_p_sorts_descending_pic_num_no_wrap() {
    let refs = [(10, meta(0, 0)), (11, meta(1, 0)), (12, meta(2, 0))];
    let list = build_default_list_p(&refs, 3, 16);
    assert_eq!(slots(&list), vec![12, 11, 10]);
}

#[test]
fn default_list_p_handles_frame_num_wrap() {
    // curr_frame_num = 1, max_frame_num = 4: refs with frame_num 2/3 wrapped to -2/-1.
    let refs = [(20, meta(2, 0)), (21, meta(3, 0))];
    let list = build_default_list_p(&refs, 1, 4);
    assert_eq!(slots(&list), vec![21, 20]);
}

#[test]
fn default_lists_b_split_and_interleave_by_poc() {
    let refs = [
        (1, meta(0, 4)),
        (2, meta(0, 8)),
        (3, meta(0, 12)),
        (4, meta(0, 16)),
    ];
    let (list0, list1) = build_default_lists_b(&refs, 0, 16, 10);
    assert_eq!(slots(&list0), vec![2, 1, 3, 4]);
    assert_eq!(slots(&list1), vec![3, 4, 2, 1]);
}

#[test]
fn default_lists_b_swaps_first_two_when_identical() {
    // Both references have POC < curr_poc, so RefPicList1 (after+before, "after" empty)
    // is identical to RefPicList0 (before+after) before the swap rule applies.
    let refs = [(1, meta(0, 2)), (2, meta(0, 4))];
    let (list0, list1) = build_default_lists_b(&refs, 0, 16, 10);
    assert_eq!(slots(&list0), vec![2, 1]);
    assert_eq!(slots(&list1), vec![1, 2]);
}

#[test]
fn apply_modifications_moves_referenced_pic_num_to_front() {
    let refs = [(10, meta(0, 0)), (11, meta(1, 0)), (12, meta(2, 0))];
    let mut list = build_default_list_p(&refs, 3, 16);
    assert_eq!(slots(&list), vec![12, 11, 10]);

    // modification_of_pic_nums_idc == 0 (subtract), abs_diff_pic_num_minus1 == 2 ->
    // picNumLXPred (CurrPicNum=3) - 3 == 0 -> picks the pic_num==0 entry (slot 10).
    let ops = [RefPicListModOp {
        add: false,
        abs_diff_pic_num_minus1: 2,
    }];
    apply_modifications(&mut list, &ops, 3, 16, 3).expect("modification should find pic_num 0");
    assert_eq!(slots(&list), vec![10, 12, 11]);
}

#[test]
fn apply_modifications_rejects_unknown_pic_num() {
    let refs = [(10, meta(0, 0))];
    let mut list = build_default_list_p(&refs, 3, 16);
    let ops = [RefPicListModOp {
        add: false,
        abs_diff_pic_num_minus1: 100,
    }];
    assert!(apply_modifications(&mut list, &ops, 3, 16, 1).is_err());
}

#[test]
fn pad_to_length_repeats_last_entry() {
    let mut list = vec![RefListEntry {
        slot: 7,
        pic_num: 0,
        poc: 0,
    }];
    pad_to_length(&mut list, 3).expect("non-empty list pads");
    assert_eq!(slots(&list), vec![7, 7, 7]);
}

#[test]
fn pad_to_length_rejects_empty_list_when_entries_needed() {
    let mut list: Vec<RefListEntry> = Vec::new();
    assert!(pad_to_length(&mut list, 1).is_err());
    assert!(pad_to_length(&mut list, 0).is_ok());
}

#[test]
fn sliding_window_evict_picks_smallest_frame_num_wrap_at_capacity() {
    let refs = [(1, meta(5, 0)), (2, meta(2, 0)), (3, meta(8, 0))];
    assert_eq!(sliding_window_evict(&refs, 10, 16, 3), Some(2));
}

#[test]
fn sliding_window_evict_none_under_capacity() {
    let refs = [(1, meta(5, 0)), (2, meta(2, 0))];
    assert_eq!(sliding_window_evict(&refs, 10, 16, 3), None);
}
