#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn new_dpb_has_no_reference() {
    let dpb = HevcDpb::new();
    assert!(dpb.reference().is_none());
}

#[test]
fn set_reference_then_clear_removes_it() {
    let mut dpb = HevcDpb::new();
    dpb.set_reference(1, 4);
    let (slot, r) = dpb.reference().expect("reference set");
    assert_eq!(*slot, 1);
    assert_eq!(r.pic_order_cnt, 4);

    dpb.clear();
    assert!(dpb.reference().is_none());
}

#[test]
fn set_reference_overwrites_previous() {
    let mut dpb = HevcDpb::new();
    dpb.set_reference(0, 0);
    dpb.set_reference(2, 8);
    let (slot, r) = dpb.reference().expect("reference set");
    assert_eq!(*slot, 2);
    assert_eq!(r.pic_order_cnt, 8);
}

#[test]
fn allocate_slot_cycles_through_pool_when_no_reference_protected() {
    let dpb = HevcDpb::new();
    let mut cursor = 0usize;
    let allocated: Vec<usize> = (0..(HEVC_SURFACE_POOL_SIZE * 2))
        .map(|_| allocate_slot(&mut cursor, &dpb))
        .collect();
    for (i, &slot) in allocated.iter().enumerate() {
        assert_eq!(slot, i % HEVC_SURFACE_POOL_SIZE);
    }
}

#[test]
fn allocate_slot_skips_the_protected_reference_slot() {
    let mut dpb = HevcDpb::new();
    dpb.set_reference(1, 0);
    let mut cursor = 1usize;
    // cursor lands on the protected slot (1) — must skip forward to 2.
    let allocated = allocate_slot(&mut cursor, &dpb);
    assert_eq!(allocated, 2);
}

#[test]
fn allocate_slot_never_returns_the_protected_slot_across_a_full_cycle() {
    let mut dpb = HevcDpb::new();
    dpb.set_reference(0, 0);
    let mut cursor = 0usize;
    for _ in 0..(HEVC_SURFACE_POOL_SIZE * 3) {
        let allocated = allocate_slot(&mut cursor, &dpb);
        assert_ne!(allocated, 0);
    }
}
