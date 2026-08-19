#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// `obu_header()` byte for a non-extended OBU with `obu_has_size_field == 1` — mirrors
/// `windows::d3d12_video_encode::bitstream_av1::obu_header_byte` (inverse direction, this
/// module's own cross-check source).
const fn obu_header_byte(obu_type: u8) -> u8 {
    (obu_type << 3) | 0b10
}

#[test]
fn leb128_single_byte() {
    assert_eq!(read_leb128(&[0x05, 0xff]).unwrap(), (5, 1));
}

#[test]
fn leb128_multi_byte_continuation() {
    // 300 = 0b1_0010_1100 -> low 7 bits 0x2C with continuation, then remaining 2.
    assert_eq!(read_leb128(&[0xAC, 0x02]).unwrap(), (300, 2));
}

#[test]
fn leb128_truncated_continuation_is_invalid() {
    assert_eq!(read_leb128(&[0x80]), Err(DecodeError::InvalidInput));
}

#[test]
fn leb128_empty_input_is_invalid() {
    assert_eq!(read_leb128(&[]), Err(DecodeError::InvalidInput));
}

#[test]
fn split_obus_temporal_delimiter_then_sequence_header() {
    let mut stream = vec![obu_header_byte(OBU_TEMPORAL_DELIMITER), 0x00];
    stream.extend_from_slice(&[obu_header_byte(OBU_SEQUENCE_HEADER), 0x02, 0xAA, 0xBB]);

    let obus = split_obus(&stream).unwrap();
    assert_eq!(obus.len(), 2);
    assert_eq!(obus[0].obu_type, OBU_TEMPORAL_DELIMITER);
    assert!(obus[0].payload.is_empty());
    assert_eq!(obus[1].obu_type, OBU_SEQUENCE_HEADER);
    assert_eq!(obus[1].payload, &[0xAA, 0xBB]);
}

#[test]
fn split_obus_rejects_forbidden_bit() {
    let stream = [0x80 | obu_header_byte(OBU_SEQUENCE_HEADER), 0x00];
    assert_eq!(split_obus(&stream), Err(DecodeError::InvalidInput));
}

#[test]
fn split_obus_rejects_extension_flag() {
    let stream = [obu_header_byte(OBU_SEQUENCE_HEADER) | 0b100, 0x00];
    assert_eq!(split_obus(&stream), Err(DecodeError::Unsupported));
}

#[test]
fn split_obus_rejects_missing_size_field() {
    let stream = [(OBU_SEQUENCE_HEADER << 3), 0x00];
    assert_eq!(split_obus(&stream), Err(DecodeError::Unsupported));
}

#[test]
fn split_obus_rejects_truncated_payload() {
    let stream = [obu_header_byte(OBU_SEQUENCE_HEADER), 0x05, 0xAA];
    assert_eq!(split_obus(&stream), Err(DecodeError::InvalidInput));
}
