#![cfg(test)]

use super::*;

#[test]
fn gop_size_one_is_always_idr() {
    let mut state = HevcGopState::new(1);
    for _ in 0..5 {
        let decision = state.decide();
        assert!(decision.is_idr);
        assert_eq!(decision.poc, 0);
    }
}

#[test]
fn gop_size_three_produces_idr_p_p_cadence() {
    let mut state = HevcGopState::new(3);
    let expect: [(bool, u32); 7] = [
        (true, 0),
        (false, 1),
        (false, 2),
        (true, 0),
        (false, 1),
        (false, 2),
        (true, 0),
    ];
    for (is_idr, poc) in expect {
        let decision = state.decide();
        assert_eq!(decision.is_idr, is_idr);
        assert_eq!(decision.poc, poc);
        assert_eq!(decision.intra_refresh_frame_index, None);
    }
}

#[test]
fn intra_refresh_only_the_first_frame_ever_is_idr() {
    let mut state = HevcGopState::new_intra_refresh(3);
    for i in 0..10u32 {
        let decision = state.decide();
        assert_eq!(decision.is_idr, i == 0, "frame {i}");
    }
}

#[test]
fn intra_refresh_wave_index_cycles_with_period_and_skips_the_idr_frame() {
    let mut state = HevcGopState::new_intra_refresh(3);
    let expect: [Option<u32>; 7] = [
        None,    // IDR
        Some(0), // wave 1
        Some(1),
        Some(2),
        Some(0), // wave 2
        Some(1),
        Some(2),
    ];
    for wave_index in expect {
        let decision = state.decide();
        assert_eq!(decision.intra_refresh_frame_index, wave_index);
    }
}
