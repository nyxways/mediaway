#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

#[test]
fn fresh_table_has_no_entries_and_size_none() {
    let table = RefTable::new();
    for slot in 0..VP9_REF_SLOTS {
        assert!(table.get(slot).is_none());
        assert!(table.size(slot).is_none());
    }
}

#[test]
fn fresh_table_free_pool_index_is_zero() {
    let table = RefTable::new();
    assert_eq!(table.free_pool_index(), 0);
}

#[test]
fn refresh_single_bit_sets_only_that_slot() {
    let mut table = RefTable::new();
    table.refresh(0b0000_0001, 3, 64, 48);
    assert_eq!(table.size(0), Some((64, 48)));
    for slot in 1..VP9_REF_SLOTS {
        assert!(table.get(slot).is_none());
    }
}

#[test]
fn refresh_multi_bit_aliases_several_slots_to_the_same_pool_index() {
    // Mirrors this crate's own VP9 encoder sibling's ping-pong output:
    // refresh_frame_flags = (1 << 1) | 0xfc = 0b1111_1110 refreshes slots 1..8.
    let mut table = RefTable::new();
    table.refresh(0b1111_1110, 5, 640, 480);
    assert!(table.get(0).is_none());
    for slot in 1..VP9_REF_SLOTS {
        let entry = table.get(slot).expect("slot should be refreshed");
        assert_eq!(entry.pool_index, 5);
        assert_eq!(entry.width, 640);
        assert_eq!(entry.height, 480);
    }
}

#[test]
fn free_pool_index_avoids_every_referenced_index() {
    let mut table = RefTable::new();
    // Reference 8 distinct pool indices across the 8 logical slots (the worst case).
    for slot in 0..VP9_REF_SLOTS {
        table.refresh(1 << slot, slot, 16, 16);
    }
    let free = table.free_pool_index();
    assert_eq!(free, POOL_SIZE - 1); // the one index (== VP9_REF_SLOTS) never referenced
    for slot in 0..VP9_REF_SLOTS {
        assert_ne!(free, table.get(slot).unwrap().pool_index);
    }
}

#[test]
fn free_pool_index_reuses_an_index_once_no_slot_references_it() {
    let mut table = RefTable::new();
    table.refresh(0xff, 0, 16, 16); // every slot -> pool index 0
    // Now only pool index 0 is referenced; every other index (1..POOL_SIZE) is free.
    let free = table.free_pool_index();
    assert_eq!(free, 1);
}

#[test]
fn get_out_of_range_slot_returns_none_not_panic() {
    let table = RefTable::new();
    assert!(table.get(VP9_REF_SLOTS + 10).is_none());
}
