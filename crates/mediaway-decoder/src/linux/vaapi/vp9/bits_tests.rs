#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

#[test]
fn s_reads_positive_zero_sign_bit() {
    // s(4): magnitude bits 0101 (5), sign bit 0 (positive) -> +5.
    let data = [0b0101_0000];
    let mut r = BitReader::new(&data);
    assert_eq!(s(&mut r, 4).unwrap(), 5);
}

#[test]
fn s_reads_negative_when_sign_bit_set() {
    // s(4): magnitude bits 0101 (5), sign bit 1 (negative) -> -5.
    let data = [0b0101_1000];
    let mut r = BitReader::new(&data);
    assert_eq!(s(&mut r, 4).unwrap(), -5);
}

#[test]
fn s_consumes_exactly_n_plus_1_bits() {
    // s(6) then one more f(1) bit — confirms s() consumes exactly 7 bits (6 magnitude + 1 sign),
    // not 6 total (which would be AV1's su(n) shape instead).
    let data = [0b1111_1111, 0b1000_0000];
    let mut r = BitReader::new(&data);
    let value = s(&mut r, 6).unwrap();
    // magnitude 0b111111 = 63, sign bit (7th bit) = 1 -> -63.
    assert_eq!(value, -63);
    // The next bit (8th overall) is the first bit of the second byte: 1.
    assert_eq!(r.read_bit().unwrap(), 1);
}

#[test]
fn s_zero_magnitude_positive_and_negative() {
    let data = [0b0000_0000];
    let mut r = BitReader::new(&data);
    assert_eq!(s(&mut r, 4).unwrap(), 0);

    let data = [0b0000_1000];
    let mut r = BitReader::new(&data);
    // Zero magnitude with sign bit set is still spec-legal (negative zero collapses to 0).
    assert_eq!(s(&mut r, 4).unwrap(), 0);
}

#[test]
fn s_errors_on_truncated_input() {
    let data: [u8; 0] = [];
    let mut r = BitReader::new(&data);
    assert!(s(&mut r, 4).is_err());
}
