//! Tests for [`super::GopState`] — sans-io, no VA-API device needed (ADR-0002 § Test plan).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

#[test]
fn gop_size_one_reproduces_all_idr_forever() {
    let mut gop = GopState::new(1);
    for expected_idr_pic_id in 0..10u16 {
        let decision = gop.decide(FrameRequest::Auto);
        assert!(decision.is_idr);
        assert_eq!(decision.frame_num, 0);
        assert_eq!(decision.poc, 0);
        assert!(decision.reference.is_none());
        assert_eq!(decision.idr_pic_id, expected_idr_pic_id);
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
fn frame_num_increments_and_resets_on_idr() {
    let mut gop = GopState::new(3);
    let d0 = gop.decide(FrameRequest::Auto); // IDR
    assert_eq!(d0.frame_num, 0);
    let d1 = gop.decide(FrameRequest::Auto); // P
    assert_eq!(d1.frame_num, 1);
    let d2 = gop.decide(FrameRequest::Auto); // P
    assert_eq!(d2.frame_num, 2);
    let d3 = gop.decide(FrameRequest::Auto); // IDR — frame_num resets to 0
    assert_eq!(d3.frame_num, 0);
}

#[test]
fn frame_num_wraps_at_max_frame_num() {
    // MaxFrameNum = 1 << (LOG2_MAX_FRAME_NUM_MINUS4 + 4) = 1 << 16 = 65536.
    // A GOP large enough to never force an IDR keeps frame_num advancing
    // every `decide()` call until it wraps back to 0.
    let max_frame_num: u32 = 1 << (LOG2_MAX_FRAME_NUM_MINUS4 + 4);
    let mut gop = GopState::new(max_frame_num + 10);
    let mut last_frame_num = 0;
    for i in 0..max_frame_num {
        let decision = gop.decide(FrameRequest::Auto);
        assert_eq!(decision.frame_num, i);
        last_frame_num = decision.frame_num;
    }
    assert_eq!(last_frame_num, max_frame_num - 1);
    // One more call wraps back to 0.
    let wrapped = gop.decide(FrameRequest::Auto);
    assert_eq!(wrapped.frame_num, 0);
}

#[test]
fn idr_pic_id_increments_once_per_idr_only() {
    // `idr_pic_id` is "only meaningful when `is_idr`" (see `FrameDecision`'s own doc) — a P
    // frame's `idr_pic_id` just carries whatever the counter currently holds (irrelevant to any
    // caller, since H.264 P slices carry no `idr_pic_id` field at all). This test only asserts
    // the sequence of values seen on IDR frames themselves.
    let mut gop = GopState::new(3);
    let mut idr_pic_ids = Vec::new();
    for _ in 0..7 {
        let decision = gop.decide(FrameRequest::Auto);
        if decision.is_idr {
            idr_pic_ids.push(decision.idr_pic_id);
        }
    }
    // 7 decide() calls at gop_size=3 produce 3 IDR frames (indices 0, 3, 6).
    assert_eq!(idr_pic_ids, vec![0, 1, 2]);
}

#[test]
fn reference_is_none_on_idr_and_some_on_p_pointing_at_preceding_setup_slot() {
    let mut gop = GopState::new(3);
    let d0 = gop.decide(FrameRequest::Auto); // IDR
    assert!(d0.reference.is_none());

    let d1 = gop.decide(FrameRequest::Auto); // P, references d0's setup_slot
    let (ref_slot, ref_dpb_slot) = d1.reference.expect("P frame must have a reference");
    assert_eq!(ref_slot, d0.setup_slot);
    assert_eq!(ref_dpb_slot.frame_num, d0.frame_num);
    assert_eq!(ref_dpb_slot.poc, d0.poc);
    assert_eq!(ref_dpb_slot.is_idr, d0.is_idr);

    let d2 = gop.decide(FrameRequest::Auto); // P, references d1's setup_slot
    let (ref_slot, ref_dpb_slot) = d2.reference.expect("P frame must have a reference");
    assert_eq!(ref_slot, d1.setup_slot);
    assert_eq!(ref_dpb_slot.frame_num, d1.frame_num);

    let d3 = gop.decide(FrameRequest::Auto); // IDR again — reference resets to None
    assert!(d3.reference.is_none());
}

#[test]
fn poc_is_twice_frame_num_pic_order_cnt_type_two() {
    let mut gop = GopState::new(4);
    for _ in 0..5 {
        let decision = gop.decide(FrameRequest::Auto);
        assert_eq!(decision.poc, 2 * i32::try_from(decision.frame_num).unwrap());
    }
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
