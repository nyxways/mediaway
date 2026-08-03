#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test module may unwrap"
)]

use super::*;

#[test]
fn frame_size_samples_computes_standard_20ms_at_48khz() {
    let samples = frame_size_samples(48_000, Rational::new(1, 50)).expect("valid duration");
    assert_eq!(samples, 960);
}

#[test]
fn frame_size_samples_computes_2_5ms_at_48khz() {
    let samples = frame_size_samples(48_000, Rational::new(1, 400)).expect("valid duration");
    assert_eq!(samples, 120);
}

#[test]
fn frame_size_samples_rejects_non_integer_division() {
    // 48000 * 1 / 3 = 16000 exactly, so pick a ratio that truly doesn't divide evenly.
    let err = frame_size_samples(48_000, Rational::new(1, 700)).unwrap_err();
    assert!(matches!(err, OpusError::InvalidFrameDuration { .. }));
}

#[test]
fn frame_size_samples_rejects_zero_denominator() {
    let err = frame_size_samples(48_000, Rational::new(1, 0)).unwrap_err();
    assert!(matches!(err, OpusError::InvalidFrameDuration { .. }));
}
