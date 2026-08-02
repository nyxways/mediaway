//! Unit tests for EBML VINT decode (sibling of `vint.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::{decode_id, decode_size, encode_id, encode_size, encode_unknown_size};
use crate::Error;

#[test]
fn size_one_byte_known() {
    let (v, len) = decode_size(&[0x82]).unwrap();
    assert_eq!(v.value, 2);
    assert!(!v.unknown);
    assert_eq!(len, 1);
}

#[test]
fn size_one_byte_unknown_marker() {
    let (v, len) = decode_size(&[0xFF]).unwrap();
    assert_eq!(v.value, 127);
    assert!(v.unknown);
    assert_eq!(len, 1);
}

#[test]
fn size_two_byte_known() {
    let (v, len) = decode_size(&[0x41, 0x02]).unwrap();
    assert_eq!(v.value, 0x102);
    assert!(!v.unknown);
    assert_eq!(len, 2);
}

#[test]
fn size_two_byte_unknown_marker() {
    let (v, len) = decode_size(&[0x7F, 0xFF]).unwrap();
    assert_eq!(v.value, 0x3FFF);
    assert!(v.unknown);
    assert_eq!(len, 2);
}

#[test]
fn size_three_byte_known() {
    let (v, len) = decode_size(&[0x20, 0x01, 0x02]).unwrap();
    assert_eq!(v.value, 0x102);
    assert!(!v.unknown);
    assert_eq!(len, 3);
}

#[test]
fn size_four_byte_known() {
    let (v, len) = decode_size(&[0x10, 0x00, 0x00, 0x05]).unwrap();
    assert_eq!(v.value, 5);
    assert!(!v.unknown);
    assert_eq!(len, 4);
}

#[test]
fn size_four_byte_unknown_marker() {
    let (v, len) = decode_size(&[0x1F, 0xFF, 0xFF, 0xFF]).unwrap();
    assert_eq!(v.value, (1u64 << 28) - 1);
    assert!(v.unknown);
    assert_eq!(len, 4);
}

#[test]
fn size_eight_byte_known() {
    let (v, len) = decode_size(&[0x01, 0, 0, 0, 0, 0, 0, 5]).unwrap();
    assert_eq!(v.value, 5);
    assert!(!v.unknown);
    assert_eq!(len, 8);
}

#[test]
fn size_eight_byte_unknown_marker() {
    // The canonical "unknown size" 8-byte EBML VINT: 01 FF FF FF FF FF FF FF.
    let (v, len) = decode_size(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]).unwrap();
    assert_eq!(v.value, (1u64 << 56) - 1);
    assert!(v.unknown);
    assert_eq!(len, 8);
}

#[test]
fn size_incomplete_on_short_buffer() {
    assert_eq!(decode_size(&[]), Err(Error::Incomplete));
    // First byte says length 2 but only 1 byte is available.
    assert_eq!(decode_size(&[0x41]), Err(Error::Incomplete));
}

#[test]
fn size_reserved_on_zero_first_byte() {
    assert_eq!(decode_size(&[0x00, 0x00]), Err(Error::ReservedVint));
}

#[test]
fn id_keeps_marker_bits_one_byte() {
    // TrackEntry (0xAE) — single-byte ID; decode_id must NOT strip the marker.
    let (id, len) = decode_id(&[0xAE]).unwrap();
    assert_eq!(id, 0xAE);
    assert_eq!(len, 1);
}

#[test]
fn id_four_byte_segment() {
    let (id, len) = decode_id(&[0x18, 0x53, 0x80, 0x67]).unwrap();
    assert_eq!(id, 0x1853_8067);
    assert_eq!(len, 4);
}

#[test]
fn id_incomplete_on_short_buffer() {
    // First byte claims 4-byte length; only 2 bytes supplied.
    assert_eq!(decode_id(&[0x1A, 0x45]), Err(Error::Incomplete));
}

#[test]
fn id_unsupported_over_four_bytes() {
    // 0x01 => 8-byte marker length, unsupported for element IDs.
    assert_eq!(
        decode_id(&[0x01, 0, 0, 0, 0, 0, 0, 0]),
        Err(Error::Unsupported("element ID longer than 4 bytes"))
    );
}

#[test]
fn encode_id_matches_decode_one_byte() {
    let mut out = Vec::new();
    encode_id(0xAE, &mut out);
    assert_eq!(out, [0xAE]);
    assert_eq!(decode_id(&out).unwrap(), (0xAE, 1));
}

#[test]
fn encode_id_matches_decode_four_byte() {
    let mut out = Vec::new();
    encode_id(0x1853_8067, &mut out);
    assert_eq!(out, [0x18, 0x53, 0x80, 0x67]);
    assert_eq!(decode_id(&out).unwrap(), (0x1853_8067, 4));
}

#[test]
fn encode_id_matches_decode_three_byte() {
    let mut out = Vec::new();
    encode_id(0x2A_D7B1, &mut out);
    assert_eq!(decode_id(&out).unwrap(), (0x2A_D7B1, 3));
}

#[test]
fn encode_size_round_trips_small_value() {
    let mut out = Vec::new();
    encode_size(2, &mut out);
    let (v, len) = decode_size(&out).unwrap();
    assert_eq!(v.value, 2);
    assert!(!v.unknown);
    assert_eq!(len, out.len());
}

#[test]
fn encode_size_round_trips_value_needing_more_bytes() {
    let mut out = Vec::new();
    encode_size(0x102, &mut out);
    let (v, len) = decode_size(&out).unwrap();
    assert_eq!(v.value, 0x102);
    assert!(!v.unknown);
    assert_eq!(len, out.len());
}

#[test]
fn encode_size_avoids_all_ones_reserved_pattern() {
    // 126 fits a 1-byte VINT's data bits (7 bits, max normal value 2^7-2=126);
    // 127 (2^7-1) is the reserved "unknown" pattern, so it must bump to 2 bytes.
    let mut out127 = Vec::new();
    encode_size(127, &mut out127);
    let (v, _) = decode_size(&out127).unwrap();
    assert_eq!(v.value, 127);
    assert!(
        !v.unknown,
        "127 must not encode as the reserved unknown-size marker"
    );
    assert_eq!(out127.len(), 2);

    let mut out126 = Vec::new();
    encode_size(126, &mut out126);
    assert_eq!(out126.len(), 1);
}

#[test]
fn encode_unknown_size_round_trips_for_each_length() {
    for len in 1u8..=8 {
        let mut out = Vec::new();
        encode_unknown_size(len, &mut out);
        assert_eq!(out.len(), len as usize);
        let (v, decoded_len) = decode_size(&out).unwrap();
        assert!(
            v.unknown,
            "length {len} unknown-size VINT must decode as unknown"
        );
        assert_eq!(decoded_len, len as usize);
    }
}
