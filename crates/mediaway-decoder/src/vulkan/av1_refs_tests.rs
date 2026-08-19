#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::{Av1RefSlots, Av1RefSlotsError};

#[test]
fn new_clamps_capacity_to_at_least_one() {
    let mut slots = Av1RefSlots::new(0);
    assert_eq!(slots.allocate_slot(), Ok(0));
}

#[test]
fn allocate_slot_picks_first_free() {
    let mut slots = Av1RefSlots::new(4);
    assert_eq!(slots.allocate_slot(), Ok(0));
    assert_eq!(slots.allocate_slot(), Ok(1));
}

#[test]
fn allocate_slot_fails_when_full() {
    let mut slots = Av1RefSlots::new(2);
    slots.allocate_slot().unwrap();
    slots.allocate_slot().unwrap();
    assert_eq!(
        slots.allocate_slot(),
        Err(Av1RefSlotsError::NoFreeSlot { capacity: 2 })
    );
}

#[test]
fn clear_all_frees_every_occupied_slot() {
    let mut slots = Av1RefSlots::new(2);
    slots.allocate_slot().unwrap();
    slots.allocate_slot().unwrap();
    slots.clear_all().unwrap();
    assert_eq!(slots.allocate_slot(), Ok(0));
}

#[test]
fn clear_all_fails_loudly_on_outstanding_handle() {
    let mut slots = Av1RefSlots::new(1);
    let index = slots.allocate_slot().unwrap();
    slots.mark_outstanding(index).unwrap();
    assert_eq!(
        slots.clear_all(),
        Err(Av1RefSlotsError::SlotOutstanding { index })
    );
}

#[test]
fn mark_outstanding_rejects_out_of_range_index() {
    let mut slots = Av1RefSlots::new(1);
    assert_eq!(
        slots.mark_outstanding(5),
        Err(Av1RefSlotsError::InvalidSlotIndex {
            index: 5,
            capacity: 1
        })
    );
}
