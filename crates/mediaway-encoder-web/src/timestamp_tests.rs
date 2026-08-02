#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test module may unwrap"
)]

use super::*;

#[test]
fn timestamp_converts_when_in_range() {
    assert_eq!(timestamp_us_to_i32(0.0), Some(0));
    assert_eq!(timestamp_us_to_i32(1_000_000.0), Some(1_000_000));
    assert_eq!(timestamp_us_to_i32(f64::from(i32::MAX)), Some(i32::MAX));
    assert_eq!(timestamp_us_to_i32(f64::from(i32::MIN)), Some(i32::MIN));
}

#[test]
fn timestamp_rejects_out_of_range() {
    assert_eq!(timestamp_us_to_i32(f64::from(i32::MAX) + 1.0), None);
    assert_eq!(timestamp_us_to_i32(f64::from(i32::MIN) - 1.0), None);
}

#[test]
fn timestamp_rejects_non_finite() {
    assert_eq!(timestamp_us_to_i32(f64::NAN), None);
    assert_eq!(timestamp_us_to_i32(f64::INFINITY), None);
    assert_eq!(timestamp_us_to_i32(f64::NEG_INFINITY), None);
}
