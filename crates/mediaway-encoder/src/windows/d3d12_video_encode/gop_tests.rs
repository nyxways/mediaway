#![cfg(test)]

use super::*;

#[test]
fn gop_size_one_is_always_idr() {
    let mut state = H264GopState::new(1);
    for _ in 0..5 {
        let decision = state.decide();
        assert!(decision.is_idr);
        assert_eq!(decision.frame_num, 0);
        assert_eq!(decision.poc, 0);
    }
}

#[test]
fn gop_size_three_produces_idr_p_p_cadence() {
    let mut state = H264GopState::new(3);
    let expect: [(bool, u32); 7] = [
        (true, 0),
        (false, 1),
        (false, 2),
        (true, 0),
        (false, 1),
        (false, 2),
        (true, 0),
    ];
    for (is_idr, frame_num) in expect {
        let decision = state.decide();
        assert_eq!(decision.is_idr, is_idr);
        assert_eq!(decision.frame_num, frame_num);
        assert_eq!(decision.poc, frame_num * 2);
    }
}
