//! Pure sans-io unit tests for [`super::read_leb128`]/[`super::split_obus`] — no
//! hardware, no D3D12 involved.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{ObuType, read_leb128, split_obus};
use crate::DecodeError;

#[test]
fn leb128_single_byte_values_round_trip() {
    assert_eq!(read_leb128(&[0x00]).unwrap(), (0, 1));
    assert_eq!(read_leb128(&[0x7F]).unwrap(), (127, 1));
}

#[test]
fn leb128_multi_byte_boundary_values_round_trip() {
    // 128 == 0x80 -> low7=0 (continue), next byte=1 (no continue).
    assert_eq!(read_leb128(&[0x80, 0x01]).unwrap(), (128, 2));
    // 300 -> low7=44 (0x2C, continue bit set -> 0xAC), next byte=2 (no continue).
    assert_eq!(read_leb128(&[0xAC, 0x02]).unwrap(), (300, 2));
    // Trailing bytes after the terminator are not consumed.
    assert_eq!(read_leb128(&[0x00, 0xFF]).unwrap(), (0, 1));
}

#[test]
fn leb128_truncated_continuation_is_invalid_input() {
    let err = read_leb128(&[0x80]).unwrap_err();
    assert!(matches!(err, DecodeError::InvalidInput));
}

#[test]
fn leb128_never_terminating_within_8_bytes_is_invalid_input() {
    let bytes = [0x80u8; 8];
    let err = read_leb128(&bytes).unwrap_err();
    assert!(matches!(err, DecodeError::InvalidInput));
}

/// `obu_header()` byte + `leb128(payload.len())` + `payload` — the read-side mirror of
/// `mediaway-encoder-windows`'s `bitstream_av1.rs::wrap_obu`.
fn wrap_obu(obu_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![(obu_type << 3) | 0b10];
    let len = u8::try_from(payload.len()).unwrap();
    out.push(len); // every test payload here is < 128 bytes, single-byte leb128
    out.extend_from_slice(payload);
    out
}

#[test]
fn split_obus_temporal_delimiter_sequence_header_and_frame() {
    let mut packet = wrap_obu(2, &[]); // OBU_TEMPORAL_DELIMITER
    packet.extend(wrap_obu(1, &[0xAA, 0xBB])); // OBU_SEQUENCE_HEADER
    packet.extend(wrap_obu(6, &[0x01, 0x02, 0x03])); // OBU_FRAME

    let obus = split_obus(&packet).unwrap();
    assert_eq!(obus.len(), 3);
    assert_eq!(obus[0].obu_type, ObuType::TemporalDelimiter);
    assert!(obus[0].payload.is_empty());
    assert_eq!(obus[1].obu_type, ObuType::SequenceHeader);
    assert_eq!(obus[1].payload, &[0xAA, 0xBB]);
    assert_eq!(obus[2].obu_type, ObuType::Frame);
    assert_eq!(obus[2].payload, &[0x01, 0x02, 0x03]);
}

#[test]
fn split_obus_empty_packet_returns_empty() {
    assert!(split_obus(&[]).unwrap().is_empty());
}

#[test]
fn split_obus_maps_every_documented_type() {
    for (raw, expected) in [
        (1u8, ObuType::SequenceHeader),
        (2, ObuType::TemporalDelimiter),
        (3, ObuType::FrameHeader),
        (4, ObuType::TileGroup),
        (6, ObuType::Frame),
        (7, ObuType::RedundantFrameHeader),
        (15, ObuType::Other(15)),
    ] {
        let packet = wrap_obu(raw, &[]);
        let obus = split_obus(&packet).unwrap();
        assert_eq!(obus[0].obu_type, expected, "raw obu_type {raw}");
    }
}

#[test]
fn split_obus_rejects_forbidden_bit_set() {
    let packet = vec![0x80, 0x00]; // forbidden_bit == 1
    let err = split_obus(&packet).unwrap_err();
    assert!(matches!(err, DecodeError::InvalidInput));
}

#[test]
fn split_obus_rejects_extension_flag() {
    // obu_type=1, obu_extension_flag=1 (bit 2), obu_has_size_field=1 (bit 1).
    let packet = vec![0b0000_1110u8];
    let err = split_obus(&packet).unwrap_err();
    assert!(matches!(err, DecodeError::Unsupported));
}

#[test]
fn split_obus_rejects_missing_size_field() {
    // obu_type=1, obu_extension_flag=0, obu_has_size_field=0.
    let packet = vec![0b0000_1000u8];
    let err = split_obus(&packet).unwrap_err();
    assert!(matches!(err, DecodeError::Unsupported));
}

#[test]
fn split_obus_rejects_truncated_payload() {
    // Claims a 5-byte payload but only 2 bytes follow.
    let packet = vec![0x0A, 0x05, 0xAA, 0xBB];
    let err = split_obus(&packet).unwrap_err();
    assert!(matches!(err, DecodeError::InvalidInput));
}
