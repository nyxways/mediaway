#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

#[test]
fn parse_accepts_disabled_segmentation() {
    let data = [0b0000_0000];
    let mut r = BitReader::new(&data);
    assert!(parse(&mut r).is_ok());
}

#[test]
fn parse_rejects_enabled_segmentation() {
    let data = [0b1000_0000];
    let mut r = BitReader::new(&data);
    assert_eq!(parse(&mut r), Err(DecodeError::Unsupported));
}

#[test]
fn parse_consumes_exactly_1_bit() {
    let data = [0b0_1000000];
    let mut r = BitReader::new(&data);
    parse(&mut r).unwrap();
    assert_eq!(r.read_bit().unwrap(), 1);
}
