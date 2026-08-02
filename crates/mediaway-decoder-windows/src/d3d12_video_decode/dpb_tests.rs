//! Pure unit tests for [`super::SlotTable`] — no D3D12/hardware involved.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::SlotTable;

#[test]
fn acquire_free_slot_exhausts_then_errors() {
    let mut table: SlotTable<u32> = SlotTable::new(2);
    let a = table.acquire_free_slot().expect("first slot free");
    let b = table.acquire_free_slot().expect("second slot free");
    assert_ne!(a, b);
    assert!(table.acquire_free_slot().is_err());
}

#[test]
fn mark_reference_is_visible_in_references() {
    let mut table: SlotTable<u32> = SlotTable::new(2);
    let slot = table.acquire_free_slot().expect("slot free");
    table.mark_reference(slot, 42);
    assert_eq!(table.references(), vec![(slot, 42)]);
}

#[test]
fn evict_fails_when_handle_outstanding() {
    let mut table: SlotTable<u32> = SlotTable::new(1);
    let slot = table.acquire_free_slot().expect("slot free");
    table.mark_reference(slot, 1);
    table.mark_handle_outstanding(slot);
    assert!(
        table.evict(slot).is_err(),
        "bounded-handle backpressure contract must reject eviction while a caller \
         may still hold a live Zero-Copy handle"
    );
}

#[test]
fn evict_succeeds_and_frees_slot_once_handle_released() {
    let mut table: SlotTable<u32> = SlotTable::new(1);
    let slot = table.acquire_free_slot().expect("slot free");
    table.mark_reference(slot, 1);
    table.mark_handle_outstanding(slot);
    assert!(table.evict(slot).is_err());

    table.release_handle(slot);
    assert!(table.evict(slot).is_ok());
    assert!(table.is_free(slot));
}

#[test]
fn release_if_unused_frees_non_reference_slot() {
    let mut table: SlotTable<u32> = SlotTable::new(1);
    let slot = table.acquire_free_slot().expect("slot free");
    assert!(!table.is_free(slot));
    table.release_if_unused(slot);
    assert!(table.is_free(slot));
}

#[test]
fn release_if_unused_keeps_active_reference_occupied() {
    let mut table: SlotTable<u32> = SlotTable::new(1);
    let slot = table.acquire_free_slot().expect("slot free");
    table.mark_reference(slot, 7);
    table.release_if_unused(slot);
    assert!(
        !table.is_free(slot),
        "still an active reference, must not be freed"
    );
}

#[test]
fn release_handle_frees_slot_when_not_a_reference() {
    let mut table: SlotTable<u32> = SlotTable::new(1);
    let slot = table.acquire_free_slot().expect("slot free");
    table.mark_handle_outstanding(slot);
    table.release_handle(slot);
    assert!(table.is_free(slot));
}

#[test]
fn release_handle_keeps_active_reference_occupied() {
    let mut table: SlotTable<u32> = SlotTable::new(1);
    let slot = table.acquire_free_slot().expect("slot free");
    table.mark_reference(slot, 9);
    table.mark_handle_outstanding(slot);
    table.release_handle(slot);
    assert!(!table.is_free(slot));
    assert_eq!(table.references(), vec![(slot, 9)]);
}
