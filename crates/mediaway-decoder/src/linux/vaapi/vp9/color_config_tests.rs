#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

#[test]
fn frame_sync_code_accepts_real_sync_bytes() {
    let data = [0x49, 0x83, 0x42];
    let mut r = BitReader::new(&data);
    assert!(frame_sync_code(&mut r).is_ok());
}

#[test]
fn frame_sync_code_rejects_wrong_bytes() {
    let data = [0x49, 0x83, 0x00];
    let mut r = BitReader::new(&data);
    assert_eq!(frame_sync_code(&mut r), Err(DecodeError::InvalidInput));
}

#[test]
fn color_config_accepts_non_rgb_color_space() {
    // color_space = CS_BT_601 (1) -> 0b001, then color_range = 1 -> 0b1, padded: 0b0011_0000.
    let data = [0b0011_0000];
    let mut r = BitReader::new(&data);
    assert!(parse(&mut r).is_ok());
}

#[test]
fn color_config_rejects_cs_rgb() {
    // color_space = CS_RGB (7) -> 0b111.
    let data = [0b1110_0000];
    let mut r = BitReader::new(&data);
    assert_eq!(parse(&mut r), Err(DecodeError::Unsupported));
}

#[test]
fn color_config_consumes_exactly_4_bits_for_non_rgb() {
    // color_space=0b001, color_range=0b1, then a marker bit 1 that must remain unread.
    let data = [0b0011_1000];
    let mut r = BitReader::new(&data);
    parse(&mut r).unwrap();
    assert_eq!(r.read_bit().unwrap(), 1);
}
