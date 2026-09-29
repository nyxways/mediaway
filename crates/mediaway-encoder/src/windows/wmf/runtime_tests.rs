#![cfg(test)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{from_hns, sample_duration_hns, to_hns};

/// Timebases a caller realistically hands this backend. Every one of them has a denominator
/// that does not divide 10 000 000, which is the whole point — that is the case the truncating
/// round trip got wrong, and picking only friendly timebases would have hidden it.
const TIMEBASES: &[(u64, u32)] = &[
    (1, 60),         // 60 fps — qarec's default capture cap, where this was found
    (1, 30),         // 30 fps
    (1, 24),         // 24 fps
    (1001, 30000),   // NTSC 29.97
    (1001, 24000),   // NTSC 23.976
    (1, 1_000),      // milliseconds
    (1, 48_000),     // AAC / WASAPI sample clock
    (1, 90_000),     // MPEG-TS
    (1, 10_000_000), // hns itself — the one case that is exact in both directions
];

#[test]
fn the_hns_round_trip_is_an_exact_inverse() {
    for &(num, den) in TIMEBASES {
        for tick in 0..2_000i64 {
            let back = from_hns(to_hns(tick, num, den), num, den);
            assert_eq!(
                back, tick,
                "tick {tick} did not survive time_base {num}/{den}"
            );
        }
    }
}

#[test]
fn it_is_an_exact_inverse_for_negative_timestamps_too() {
    // MF sample times are non-negative in practice, but `to_hns` truncates *toward* zero, so
    // the two signs need opposite rounding. Asserting only one side would leave the other free
    // to be off by a tick without a test noticing.
    for &(num, den) in TIMEBASES {
        for tick in -2_000..0i64 {
            let back = from_hns(to_hns(tick, num, den), num, den);
            assert_eq!(
                back, tick,
                "tick {tick} did not survive time_base {num}/{den}"
            );
        }
    }
}

#[test]
fn distinct_ticks_never_share_a_timestamp() {
    // The defect as a recording actually showed it: strictly increasing input ticks came back
    // with duplicates, and a video track with two frames on one instant is malformed. This is
    // the property the file needed, stated directly rather than via the round trip.
    for &(num, den) in TIMEBASES {
        let mut previous = from_hns(to_hns(0, num, den), num, den);
        for tick in 1..2_000i64 {
            let current = from_hns(to_hns(tick, num, den), num, den);
            assert!(
                current > previous,
                "time_base {num}/{den}: tick {tick} came back as {current}, \
                 not greater than the previous {previous}"
            );
            previous = current;
        }
    }
}

#[test]
fn the_measured_regression_case_survives() {
    // The exact numbers from the 2026-09-18 recording: at 1/60 every tick that is not a
    // multiple of three used to come back as its predecessor.
    assert_eq!(to_hns(7, 1, 60), 1_166_666);
    assert_eq!(from_hns(1_166_666, 1, 60), 7);
    assert_eq!(to_hns(8, 1, 60), 1_333_333);
    assert_eq!(from_hns(1_333_333, 1, 60), 8);
}

#[test]
fn a_timestamp_the_mft_recomputed_lands_on_the_tick_it_meant() {
    // An MFT that recomputes sample times (the inbox H.264 one does) reports a whole hns near
    // the exact value rather than the one written to it. Measured MFTs landed at or below the
    // exact value; this also covers one rounding *up*, which is the case that separates
    // nearest from away-from-zero. See `from_hns` — this is the design argument, not a
    // measured failure.
    for tick in 1..1_000i64 {
        // The exact hns for this tick is `tick * 10_000_000 / 60`, which is not a whole
        // number. An MFT reports one of the two whole hns values bracketing it; both must
        // come back as `tick`. Done in integers — a float here would introduce a rounding
        // error of its own and test the wrong thing.
        let scaled = tick * 10_000_000;
        let from_below = scaled / 60;
        let from_above = (scaled + 59) / 60;
        assert_eq!(from_hns(from_above, 1, 60), tick, "rounded up by the MFT");
        assert_eq!(from_hns(from_below, 1, 60), tick, "rounded down by the MFT");
    }
}

#[test]
fn a_timebase_finer_than_mf_can_represent_loses_precision_on_the_way_in() {
    // 20 MHz — half an hns per tick, past the condition `from_hns` documents. The loss is real
    // and is recorded here rather than papered over, but note *where* it happens: two distinct
    // ticks already collide in `to_hns`, before any rounding rule on the way back could help.
    let (num, den) = (1, 20_000_000);
    assert_eq!(to_hns(1, num, den), to_hns(0, num, den));
}

#[test]
fn a_zero_timebase_yields_zero_instead_of_dividing_by_it() {
    assert_eq!(to_hns(42, 1, 0), 0);
    assert_eq!(from_hns(42, 1, 0), 0);
    assert_eq!(from_hns(42, 0, 60), 0);
}

/// A frame with no duration must not be handed to the MFT as a 100-nanosecond sample: it is one
/// tick, the nominal frame interval, the same as declaring `duration: 1`.
#[test]
fn unknown_duration_is_one_tick_not_one_hns() {
    for &(num, den) in TIMEBASES {
        assert_eq!(
            sample_duration_hns(0, num, den),
            sample_duration_hns(1, num, den),
            "duration 0 differs from one tick at {num}/{den}"
        );
    }
    // At 1/30 a tick is 333 333 hns; the old code answered 1.
    assert_eq!(sample_duration_hns(0, 1, 30), 333_333);
}

/// A duration the caller did give is honoured, tick for tick.
#[test]
fn declared_duration_is_kept() {
    // Truncated once, over the whole duration, not once per tick.
    assert_eq!(sample_duration_hns(3, 1, 30), 1_000_000);
    assert_eq!(sample_duration_hns(2, 1001, 30000), to_hns(2, 1001, 30000));
}

/// Not representable as `i64`: unknown, not zero and not a wrapped negative.
#[test]
fn an_oversized_duration_is_treated_as_unknown() {
    assert_eq!(
        sample_duration_hns(u64::MAX, 1, 30),
        sample_duration_hns(1, 1, 30)
    );
}

/// A time base finer than one hns still yields a positive duration; MF rejects zero.
#[test]
fn a_sub_hns_tick_is_floored_at_one() {
    assert_eq!(sample_duration_hns(0, 1, 100_000_000), 1);
}
