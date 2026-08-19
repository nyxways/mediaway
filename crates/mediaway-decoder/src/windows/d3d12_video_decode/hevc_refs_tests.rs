//! Pure unit tests for [`super::slots_to_evict`]/[`super::build_ref_lists`] against a
//! synthetic DPB (`Vec<(u32, HevcRefMeta)>`, no D3D12 device involved — same
//! device-free testing `SlotTable<M>`'s own `dpb_tests.rs` already enables).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{HevcRefMeta, build_ref_lists, slots_to_evict};

fn meta(poc: i32) -> HevcRefMeta {
    HevcRefMeta { poc }
}

#[test]
fn slots_to_evict_keeps_slots_named_in_rps_evicts_others() {
    let refs = vec![(0u32, meta(2)), (1u32, meta(4)), (2u32, meta(6))];
    // Current picture's RPS only names POC 2 and 6 (either before or after, or a
    // "foll" entry not currently used) -> POC 4's slot (index 1) must be evicted.
    let all_rps_poc = [2, 6];
    let evicted = slots_to_evict(&refs, &all_rps_poc);
    assert_eq!(evicted.as_slice(), [1]);
}

#[test]
fn slots_to_evict_evicts_everything_when_rps_is_empty() {
    let refs = vec![(0u32, meta(2)), (1u32, meta(4))];
    let evicted = slots_to_evict(&refs, &[]);
    let mut evicted_sorted = evicted.into_vec();
    evicted_sorted.sort_unstable();
    assert_eq!(evicted_sorted, vec![0, 1]);
}

#[test]
fn slots_to_evict_keeps_everything_when_rps_names_every_slot() {
    let refs = vec![(0u32, meta(2)), (1u32, meta(4))];
    let evicted = slots_to_evict(&refs, &[2, 4]);
    assert!(evicted.is_empty());
}

#[test]
fn build_ref_lists_single_forward_reference_happy_path() {
    let refs = vec![(3u32, meta(10))];
    let before = [10i32];
    let after: [i32; 0] = [];
    let lists = build_ref_lists(&refs, &before, &after).expect("single ref in DPB");
    assert_eq!(lists.ref_pic_list.as_slice(), [3]);
    assert_eq!(lists.poc_list.as_slice(), [10]);
    // POC 10 is `ref_pic_list`'s index 0 -> RefPicSetStCurrBefore holds byte-index 0
    // (ADR-0004's believed RefPicList-index semantics, see hevc_refs.rs's module doc).
    assert_eq!(lists.st_curr_before.as_slice(), [0]);
    assert!(lists.st_curr_after.is_empty());
}

#[test]
fn build_ref_lists_after_entry_indexes_correctly_among_multiple_refs() {
    let refs = vec![(0u32, meta(5)), (1u32, meta(8)), (2u32, meta(15))];
    let before: [i32; 0] = [];
    let after = [15i32];
    let lists = build_ref_lists(&refs, &before, &after).expect("POC 15 is in the DPB");
    assert!(lists.st_curr_before.is_empty());
    // POC 15 is `ref_pic_list`'s index 2 (refs are walked in `refs`' own order).
    assert_eq!(lists.st_curr_after.as_slice(), [2]);
}

#[test]
fn build_ref_lists_errors_when_referenced_poc_is_not_in_dpb() {
    let refs = vec![(0u32, meta(10))];
    let before = [99i32]; // not present in `refs`
    let after: [i32; 0] = [];
    let result = build_ref_lists(&refs, &before, &after);
    assert!(result.is_err());
}
