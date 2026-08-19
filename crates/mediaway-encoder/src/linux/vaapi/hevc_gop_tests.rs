//! Tests for [`super::GopState`] (HEVC) — sans-io, no VA-API device needed (ADR-0003 § Test
//! plan). Mirrors `gop_tests.rs`'s H.264 coverage shape, minus the `frame_num`/`idr_pic_id`
//! specific cases (HEVC has neither) and the wraparound test (this port never wraps `poc`, see
//! this module's own doc comment).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

#[test]
fn gop_size_one_reproduces_all_idr_forever() {
    let mut gop = GopState::new(1);
    for _ in 0..10 {
        let decision = gop.decide(FrameRequest::Auto);
        assert!(decision.is_idr);
        assert_eq!(decision.poc, 0);
        assert!(decision.reference.is_none());
    }
}

#[test]
fn gop_size_three_produces_ippippi_cadence_over_seven_frames() {
    let mut gop = GopState::new(3);
    let expected_is_idr = [true, false, false, true, false, false, true];
    for expected in expected_is_idr {
        let decision = gop.decide(FrameRequest::Auto);
        assert_eq!(decision.is_idr, expected);
    }
}

#[test]
fn poc_increments_by_one_and_resets_on_idr() {
    let mut gop = GopState::new(3);
    let d0 = gop.decide(FrameRequest::Auto); // IDR
    assert_eq!(d0.poc, 0);
    let d1 = gop.decide(FrameRequest::Auto); // P
    assert_eq!(d1.poc, 1);
    let d2 = gop.decide(FrameRequest::Auto); // P
    assert_eq!(d2.poc, 2);
    let d3 = gop.decide(FrameRequest::Auto); // IDR again — poc resets to 0
    assert_eq!(d3.poc, 0);
}

#[test]
fn reference_is_none_on_idr_and_some_on_p_pointing_at_preceding_setup_slot() {
    let mut gop = GopState::new(3);
    let d0 = gop.decide(FrameRequest::Auto); // IDR
    assert!(d0.reference.is_none());

    let d1 = gop.decide(FrameRequest::Auto); // P, references d0's setup_slot
    let (ref_slot, ref_dpb_slot) = d1.reference.expect("P frame must have a reference");
    assert_eq!(ref_slot, d0.setup_slot);
    assert_eq!(ref_dpb_slot.poc, d0.poc);

    let d2 = gop.decide(FrameRequest::Auto); // P, references d1's setup_slot
    let (ref_slot, ref_dpb_slot) = d2.reference.expect("P frame must have a reference");
    assert_eq!(ref_slot, d1.setup_slot);
    assert_eq!(ref_dpb_slot.poc, d1.poc);

    let d3 = gop.decide(FrameRequest::Auto); // IDR again — reference resets to None
    assert!(d3.reference.is_none());
}

#[test]
fn setup_slot_cycles_through_workspace_dpb_cap_slots() {
    let mut gop = GopState::new(100); // large enough that no IDR forces mid-run
    let mut slots = Vec::new();
    for _ in 0..(WORKSPACE_DPB_CAP * 2) {
        slots.push(gop.decide(FrameRequest::Auto).setup_slot);
    }
    for (i, &slot) in slots.iter().enumerate() {
        assert_eq!(slot, i % WORKSPACE_DPB_CAP);
    }
}
