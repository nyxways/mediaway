//! Unit tests for PES header build/parse.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::{build_pes_header, parse_pes_header, stream_id_for};
use crate::error::Error;

#[test]
fn roundtrips_pts_only() {
    let header = build_pes_header(stream_id_for(false), 100, 90_000, None);
    let parsed = parse_pes_header(&header).unwrap();
    assert_eq!(parsed.pts_90k, 90_000);
    assert_eq!(parsed.dts_90k, None);
    assert_eq!(parsed.header_len, header.len());
}

#[test]
fn roundtrips_pts_and_dts() {
    let header = build_pes_header(stream_id_for(true), 200, 180_000, Some(177_000));
    let parsed = parse_pes_header(&header).unwrap();
    assert_eq!(parsed.pts_90k, 180_000);
    assert_eq!(parsed.dts_90k, Some(177_000));
    assert_eq!(parsed.header_len, header.len());
}

#[test]
fn roundtrips_max_33_bit_timestamp() {
    let max_33_bit = (1u64 << 33) - 1;
    let header = build_pes_header(stream_id_for(false), 10, max_33_bit, None);
    let parsed = parse_pes_header(&header).unwrap();
    assert_eq!(parsed.pts_90k, max_33_bit);
}

#[test]
fn pes_packet_length_field_matches_payload() {
    let header = build_pes_header(stream_id_for(false), 50, 0, None);
    let declared = u16::from_be_bytes([header[4], header[5]]);
    // after PES_packet_length field: 2 flag bytes + 1 header_data_length byte + 5 PTS bytes + 50 payload = 58
    assert_eq!(declared, 58);
}

#[test]
fn rejects_bad_start_code() {
    let bad = [0x00, 0x00, 0x02, 0xE0, 0, 0, 0x80, 0x00, 0x00];
    assert!(matches!(
        parse_pes_header(&bad),
        Err(Error::BadPesStartCode)
    ));
}
