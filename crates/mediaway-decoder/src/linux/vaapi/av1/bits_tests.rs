#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn uvlc_zero_is_single_stop_bit() {
    let mut r = BitReader::new(&[0b1000_0000]);
    assert_eq!(uvlc(&mut r).unwrap(), 0);
    assert_eq!(r.bits_read(), 1);
}

#[test]
fn uvlc_decodes_one_leading_zero_values() {
    let mut r = BitReader::new(&[0b0100_0000]); // "010" -> 1
    assert_eq!(uvlc(&mut r).unwrap(), 1);

    let mut r = BitReader::new(&[0b0110_0000]); // "011" -> 2
    assert_eq!(uvlc(&mut r).unwrap(), 2);
}

#[test]
fn uvlc_decodes_two_leading_zero_value() {
    let mut r = BitReader::new(&[0b0011_0000]); // "00110" -> 5
    assert_eq!(uvlc(&mut r).unwrap(), 5);
}

#[test]
fn uvlc_truncated_input_is_invalid() {
    let mut r = BitReader::new(&[]);
    assert_eq!(uvlc(&mut r), Err(DecodeError::InvalidInput));
}

#[test]
fn su_reads_negative_top_bit_set() {
    let mut r = BitReader::new(&[0b1000_0000]); // su(7): value=64, sign set -> -64
    assert_eq!(su(&mut r, 7).unwrap(), -64);
}

#[test]
fn su_reads_zero() {
    let mut r = BitReader::new(&[0b0000_0000]);
    assert_eq!(su(&mut r, 7).unwrap(), 0);
}

#[test]
fn su_reads_positive_max_magnitude() {
    let mut r = BitReader::new(&[0b0111_1110]); // su(7): value=63, top bit unset -> 63
    assert_eq!(su(&mut r, 7).unwrap(), 63);
}

#[test]
fn su_reads_negative_near_min() {
    let mut r = BitReader::new(&[0b1000_0010]); // su(7): value=65 -> 65 - 128 = -63
    assert_eq!(su(&mut r, 7).unwrap(), -63);
}

#[test]
fn ns_direct_range_values() {
    let mut r = BitReader::new(&[0b0000_0000]); // "00" -> 0
    assert_eq!(ns(&mut r, 5).unwrap(), 0);

    let mut r = BitReader::new(&[0b0100_0000]); // "01" -> 1
    assert_eq!(ns(&mut r, 5).unwrap(), 1);

    let mut r = BitReader::new(&[0b1000_0000]); // "10" -> 2
    assert_eq!(ns(&mut r, 5).unwrap(), 2);
}

#[test]
fn ns_extended_range_values() {
    let mut r = BitReader::new(&[0b1100_0000]); // "110" -> 3
    assert_eq!(ns(&mut r, 5).unwrap(), 3);
    assert_eq!(r.bits_read(), 3);

    let mut r = BitReader::new(&[0b1110_0000]); // "111" -> 4
    assert_eq!(ns(&mut r, 5).unwrap(), 4);
}

#[test]
fn ns_of_one_reads_nothing() {
    let mut r = BitReader::new(&[]);
    assert_eq!(ns(&mut r, 1).unwrap(), 0);
    assert_eq!(r.bits_read(), 0);
}
