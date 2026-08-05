#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

fn sample_header() -> RtpHeader {
    RtpHeader {
        marker: true,
        payload_type: 96,
        sequence_number: 0x1234,
        timestamp: 0xDEAD_BEEF,
        ssrc: 0x1122_3344,
    }
}

#[test]
fn header_round_trips_through_write_and_parse() {
    let header = sample_header();
    let mut bytes = Vec::new();
    header.write(&mut bytes).expect("write");
    assert_eq!(bytes.len(), HEADER_LEN);

    let (parsed, consumed) = RtpHeader::parse(&bytes).expect("parse");
    assert_eq!(consumed, HEADER_LEN);
    assert_eq!(parsed, header);
}

#[test]
fn write_rejects_out_of_range_payload_type() {
    let mut header = sample_header();
    header.payload_type = 200; // top bit set — doesn't fit 7 bits
    let mut bytes = Vec::new();
    assert!(matches!(
        header.write(&mut bytes),
        Err(Error::PayloadTypeOutOfRange(200))
    ));
}

#[test]
fn write_encodes_version_2_and_no_padding_extension_csrc() {
    let header = sample_header();
    let mut bytes = Vec::new();
    header.write(&mut bytes).expect("write");
    assert_eq!(bytes[0], 0b1000_0000); // V=2, P=0, X=0, CC=0
}

#[test]
fn write_encodes_marker_and_payload_type_byte() {
    let mut header = sample_header();
    header.marker = false;
    header.payload_type = 96;
    let mut bytes = Vec::new();
    header.write(&mut bytes).expect("write");
    assert_eq!(bytes[1], 96); // marker clear, PT=96

    header.marker = true;
    bytes.clear();
    header.write(&mut bytes).expect("write");
    assert_eq!(bytes[1], 0x80 | 0x60); // marker set, PT=96 (0x60)
}

#[test]
fn parse_rejects_short_buffer() {
    let err = RtpHeader::parse(&[0u8; HEADER_LEN - 1]).unwrap_err();
    assert!(matches!(
        err,
        Error::BufferTooShort {
            needed: HEADER_LEN,
            got
        } if got == HEADER_LEN - 1
    ));
}

#[test]
fn parse_rejects_unsupported_version() {
    let mut bytes = vec![0u8; HEADER_LEN];
    bytes[0] = 0b0100_0000; // V=1
    let err = RtpHeader::parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::UnsupportedRtpVersion(1)));
}

#[test]
fn parse_rejects_padding_bit_set() {
    let mut bytes = vec![0u8; HEADER_LEN];
    bytes[0] = 0b1010_0000; // V=2, P=1
    let err = RtpHeader::parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::PaddingUnsupported));
}

#[test]
fn parse_rejects_extension_bit_set() {
    let mut bytes = vec![0u8; HEADER_LEN];
    bytes[0] = 0b1001_0000; // V=2, X=1
    let err = RtpHeader::parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::HeaderExtensionUnsupported));
}

#[test]
fn parse_skips_csrc_list_without_exposing_it() {
    let mut bytes = vec![0u8; HEADER_LEN];
    bytes[0] = 0b1000_0010; // V=2, CC=2
    bytes.extend_from_slice(&[0xAA; 8]); // two 4-byte CSRC entries
    let (_, consumed) = RtpHeader::parse(&bytes).expect("parse");
    assert_eq!(consumed, HEADER_LEN + 8);
}

#[test]
fn parse_rejects_buffer_too_short_for_declared_csrc_count() {
    let mut bytes = vec![0u8; HEADER_LEN];
    bytes[0] = 0b1000_0001; // V=2, CC=1, but no CSRC bytes follow
    let err = RtpHeader::parse(&bytes).unwrap_err();
    assert!(matches!(
        err,
        Error::BufferTooShort {
            needed,
            got
        } if needed == HEADER_LEN + 4 && got == HEADER_LEN
    ));
}

#[test]
fn packet_round_trips_through_write_and_parse() {
    let packet = RtpPacket {
        header: sample_header(),
        payload: Bytes::from_static(&[1, 2, 3, 4, 5]),
    };
    let mut bytes = Vec::new();
    packet.write(&mut bytes).expect("write");
    assert_eq!(bytes.len(), HEADER_LEN + 5);

    let parsed = RtpPacket::parse(&bytes).expect("parse");
    assert_eq!(parsed, packet);
}
