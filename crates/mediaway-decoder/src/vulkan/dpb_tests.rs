#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::{Dpb, DpbError, DpbSlot, H264_MAX_DPB_SLOTS, compute_frame_num_wrap};

#[test]
fn new_clamps_capacity_to_at_least_one() {
    let dpb = Dpb::new(0);
    assert_eq!(dpb.capacity(), 1);
}

#[test]
fn new_clamps_capacity_to_h264_max() {
    let dpb = Dpb::new(1000);
    assert_eq!(dpb.capacity(), H264_MAX_DPB_SLOTS);
}

#[test]
fn free_slot_index_picks_first_free() {
    let mut dpb = Dpb::new(4);
    dpb.insert(0, DpbSlot::new_reference(0, 0, 0)).unwrap();
    dpb.insert(2, DpbSlot::new_reference(2, 2, 4)).unwrap();
    assert_eq!(dpb.free_slot_index(), Some(1));
}

#[test]
fn allocate_slot_reuses_free_before_evicting() {
    let mut dpb = Dpb::new(2);
    dpb.insert(0, DpbSlot::new_reference(0, 0, 0)).unwrap();
    let allocated = dpb.allocate_slot().unwrap();
    assert_eq!(allocated, 1);
    // The occupied slot 0 must be untouched — allocate_slot only reused the
    // free slot, no eviction should have happened.
    assert!(dpb.slot(0).is_some());
}

#[test]
fn allocate_slot_evicts_smallest_frame_num_wrap_when_full() {
    let mut dpb = Dpb::new(2);
    dpb.insert(0, DpbSlot::new_reference(5, 5, 10)).unwrap();
    dpb.insert(1, DpbSlot::new_reference(2, 2, 4)).unwrap();
    let allocated = dpb.allocate_slot().unwrap();
    // Slot 1 has the smaller frame_num_wrap (2 < 5) — sliding window evicts
    // the oldest reference first.
    assert_eq!(allocated, 1);
    assert!(dpb.slot(1).is_none());
    assert!(dpb.slot(0).is_some());
}

#[test]
fn allocate_slot_ignores_non_reference_slots_for_eviction() {
    let mut dpb = Dpb::new(1);
    let mut non_ref = DpbSlot::new_reference(0, 0, 0);
    non_ref.used_for_reference = false;
    dpb.insert(0, non_ref).unwrap();
    // No reference slot exists to evict via sliding window, and no free slot
    // exists either (capacity 1, occupied by a non-reference picture) — this
    // is a real edge case a caller must handle explicitly rather than the
    // DPB silently clobbering a non-reference (still-pending-output) picture.
    let err = dpb.allocate_slot().unwrap_err();
    assert_eq!(err, DpbError::NoFreeSlot { capacity: 1 });
}

#[test]
fn insert_into_outstanding_slot_fails_loudly() {
    let mut dpb = Dpb::new(2);
    dpb.insert(0, DpbSlot::new_reference(0, 0, 0)).unwrap();
    dpb.mark_outstanding(0).unwrap();
    let err = dpb.insert(0, DpbSlot::new_reference(1, 1, 2)).unwrap_err();
    assert_eq!(err, DpbError::SlotOutstanding { index: 0 });
}

#[test]
fn evict_outstanding_slot_fails_loudly() {
    let mut dpb = Dpb::new(2);
    dpb.insert(0, DpbSlot::new_reference(0, 0, 0)).unwrap();
    dpb.mark_outstanding(0).unwrap();
    let err = dpb.evict(0).unwrap_err();
    assert_eq!(err, DpbError::SlotOutstanding { index: 0 });
    // Never silently overwritten: the slot must still be occupied afterward.
    assert!(dpb.slot(0).is_some());
}

#[test]
fn clear_outstanding_allows_eviction_again() {
    let mut dpb = Dpb::new(2);
    dpb.insert(0, DpbSlot::new_reference(0, 0, 0)).unwrap();
    dpb.mark_outstanding(0).unwrap();
    assert!(dpb.evict(0).is_err());
    dpb.clear_outstanding(0).unwrap();
    dpb.evict(0).unwrap();
    assert!(dpb.slot(0).is_none());
}

#[test]
fn out_of_range_index_reports_invalid_slot_index() {
    let mut dpb = Dpb::new(2);
    let err = dpb.insert(5, DpbSlot::new_reference(0, 0, 0)).unwrap_err();
    assert_eq!(
        err,
        DpbError::InvalidSlotIndex {
            index: 5,
            capacity: 2
        }
    );
    assert_eq!(
        dpb.mark_outstanding(9).unwrap_err(),
        DpbError::InvalidSlotIndex {
            index: 9,
            capacity: 2
        }
    );
}

#[test]
fn clear_all_evicts_every_occupied_slot() {
    let mut dpb = Dpb::new(4);
    dpb.insert(0, DpbSlot::new_reference(0, 0, 0)).unwrap();
    dpb.insert(2, DpbSlot::new_reference(2, 2, 4)).unwrap();
    dpb.clear_all().unwrap();
    assert!(dpb.slot(0).is_none());
    assert!(dpb.slot(2).is_none());
}

#[test]
fn clear_all_fails_loudly_on_outstanding_slot() {
    let mut dpb = Dpb::new(2);
    dpb.insert(0, DpbSlot::new_reference(0, 0, 0)).unwrap();
    dpb.mark_outstanding(0).unwrap();
    let err = dpb.clear_all().unwrap_err();
    assert_eq!(err, DpbError::SlotOutstanding { index: 0 });
}

#[test]
fn is_outstanding_defaults_false_and_out_of_range_is_false() {
    let dpb = Dpb::new(2);
    assert!(!dpb.is_outstanding(0));
    assert!(!dpb.is_outstanding(99));
}

#[test]
fn occupied_slots_reports_index_and_slot_in_order() {
    let mut dpb = Dpb::new(3);
    dpb.insert(2, DpbSlot::new_reference(2, 2, 4)).unwrap();
    dpb.insert(0, DpbSlot::new_reference(0, 0, 0)).unwrap();
    let occupied: Vec<_> = dpb
        .occupied_slots()
        .map(|(i, s)| (i, s.frame_num))
        .collect();
    assert_eq!(occupied, vec![(0, 0), (2, 2)]);
}

#[test]
fn sliding_window_evict_target_none_when_empty() {
    let dpb = Dpb::new(4);
    assert_eq!(dpb.sliding_window_evict_target(), None);
}

#[test]
fn refresh_frame_num_wraps_recomputes_relative_to_current_picture() {
    let mut dpb = Dpb::new(4);
    // Inserted when frame_num=14 was itself "current" (frame_num_wrap == 14).
    dpb.insert(0, DpbSlot::new_reference(14, 14, 28)).unwrap();
    // New current picture wraps around: frame_num=1, max_frame_num=16.
    // frame_num(14) > current_frame_num(1) => FrameNumWrap = 14 - 16 = -2.
    dpb.refresh_frame_num_wraps(1, 16);
    assert_eq!(dpb.slot(0).unwrap().frame_num_wrap, -2);
}

#[test]
fn compute_frame_num_wrap_no_wrap_when_not_greater() {
    // frame_num <= current_frame_num: unchanged.
    assert_eq!(compute_frame_num_wrap(3, 5, 16), 3);
    assert_eq!(compute_frame_num_wrap(5, 5, 16), 5);
}

#[test]
fn compute_frame_num_wrap_wraps_when_greater() {
    // frame_num > current_frame_num: subtract MaxFrameNum.
    assert_eq!(compute_frame_num_wrap(15, 2, 16), 15 - 16);
}
