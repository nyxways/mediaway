//! Tests for [`super::GopState`] — sans-io, no VA-API device needed (encoder ADR-0004 § Test
//! plan).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

#[test]
fn gop_size_one_reproduces_all_key_forever() {
    let mut gop = GopState::new(1);
    for _ in 0..10 {
        let decision = gop.decide(FrameRequest::Auto);
        assert!(decision.is_key);
        assert_eq!(decision.setup_slot, 0);
        assert_eq!(decision.refresh_frame_flags, 0xff);
        assert!(decision.reference_slot.is_none());
    }
}

#[test]
fn gop_size_three_produces_kppkppk_cadence_over_seven_frames() {
    let mut gop = GopState::new(3);
    let expected_is_key = [true, false, false, true, false, false, true];
    for expected in expected_is_key {
        let decision = gop.decide(FrameRequest::Auto);
        assert_eq!(decision.is_key, expected);
    }
}

#[test]
fn setup_slot_alternates_on_every_p_frame_never_repeating_consecutively() {
    let mut gop = GopState::new(100); // large enough that no key frame forces mid-run
    let mut slots = Vec::new();
    for _ in 0..8 {
        slots.push(gop.decide(FrameRequest::Auto).setup_slot);
    }
    // First frame (key) writes slot 0; every P frame afterward alternates 1, 0, 1, 0, ...
    assert_eq!(slots, vec![0, 1, 0, 1, 0, 1, 0, 1]);
    for pair in slots.windows(2) {
        assert_ne!(pair[0], pair[1]);
    }
}

#[test]
fn refresh_frame_flags_is_0xff_on_key_and_masked_on_p() {
    let mut gop = GopState::new(3);
    let key = gop.decide(FrameRequest::Auto);
    assert_eq!(key.refresh_frame_flags, 0xff);

    let p1 = gop.decide(FrameRequest::Auto);
    // setup_slot alternates from key's slot 0 -> p1's slot 1: (1 << 1) | 0xfc = 0xfe.
    assert_eq!(p1.setup_slot, 1);
    assert_eq!(p1.refresh_frame_flags, 0xfe);

    let p2 = gop.decide(FrameRequest::Auto);
    // p2's slot 0: (1 << 0) | 0xfc = 0xfd.
    assert_eq!(p2.setup_slot, 0);
    assert_eq!(p2.refresh_frame_flags, 0xfd);
}

#[test]
fn reference_slot_is_none_on_key_and_the_other_ping_pong_slot_on_p() {
    let mut gop = GopState::new(3);
    let d0 = gop.decide(FrameRequest::Auto); // key, slot 0
    assert!(d0.reference_slot.is_none());

    let d1 = gop.decide(FrameRequest::Auto); // P, slot 1, references slot 0
    assert_eq!(d1.reference_slot, Some(d0.setup_slot));

    let d2 = gop.decide(FrameRequest::Auto); // P, slot 0, references slot 1
    assert_eq!(d2.reference_slot, Some(d1.setup_slot));

    let d3 = gop.decide(FrameRequest::Auto); // key again — reference resets to None
    assert!(d3.reference_slot.is_none());
}

#[test]
fn force_key_request_produces_a_key_frame_mid_gop() {
    let mut gop = GopState::new(10);
    let _ = gop.decide(FrameRequest::Auto); // key (frames_since_key == 0)
    let _ = gop.decide(FrameRequest::Auto); // P
    let forced = gop.decide(FrameRequest::ForceKey);
    assert!(forced.is_key);
    assert_eq!(forced.setup_slot, 0);
    assert_eq!(forced.refresh_frame_flags, 0xff);
    assert!(forced.reference_slot.is_none());
}
