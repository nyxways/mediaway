//! Unit tests for TS packet write/parse.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::{PACKET_LEN, parse_ts_packet, write_ts_packets};

fn packets_of(bytes: &[u8]) -> Vec<&[u8]> {
    assert_eq!(bytes.len() % PACKET_LEN, 0);
    bytes.chunks(PACKET_LEN).collect()
}

#[test]
fn short_payload_pads_to_exactly_one_packet() {
    let mut out = Vec::new();
    let mut cc = 0u8;
    write_ts_packets(&mut out, 100, &mut cc, b"hello", true, false);
    assert_eq!(out.len(), PACKET_LEN);
}

#[test]
fn exact_184_byte_payload_needs_no_adaptation_field() {
    let mut out = Vec::new();
    let mut cc = 0u8;
    let payload = vec![0xAB; 184];
    write_ts_packets(&mut out, 100, &mut cc, &payload, true, false);
    assert_eq!(out.len(), PACKET_LEN);
    let afc = (out[3] >> 4) & 0x03;
    assert_eq!(afc, 0b01); // payload-only, no adaptation field
}

#[test]
fn long_payload_spans_multiple_packets_with_pusi_only_on_first() {
    let mut out = Vec::new();
    let mut cc = 0u8;
    let payload = vec![0x11u8; 500]; // > 184, needs 3 packets
    write_ts_packets(&mut out, 256, &mut cc, &payload, true, false);
    let packets = packets_of(&out);
    assert_eq!(packets.len(), 3);

    let first = parse_ts_packet(packets[0]).unwrap();
    assert!(first.pusi);
    let second = parse_ts_packet(packets[1]).unwrap();
    assert!(!second.pusi);
    let third = parse_ts_packet(packets[2]).unwrap();
    assert!(!third.pusi);
}

#[test]
fn roundtrips_payload_across_packets() {
    let mut out = Vec::new();
    let mut cc = 0u8;
    let payload: Vec<u8> = (0..500u32)
        .map(|i| u8::try_from(i % 256).unwrap_or(0))
        .collect();
    write_ts_packets(&mut out, 33, &mut cc, &payload, true, false);

    let mut reassembled = Vec::new();
    for packet in packets_of(&out) {
        let parsed = parse_ts_packet(packet).unwrap();
        assert_eq!(parsed.pid, 33);
        reassembled.extend_from_slice(parsed.payload);
    }
    assert_eq!(reassembled, payload);
}

#[test]
fn random_access_flag_survives_roundtrip_on_first_packet() {
    let mut out = Vec::new();
    let mut cc = 0u8;
    write_ts_packets(&mut out, 1, &mut cc, b"keyframe data", true, true);

    let packets = packets_of(&out);
    let first = parse_ts_packet(packets[0]).unwrap();
    assert!(first.random_access);
}

#[test]
fn continuity_counter_increments_and_wraps() {
    let mut out = Vec::new();
    let mut cc = 14u8; // near wraparound
    write_ts_packets(&mut out, 1, &mut cc, b"a", false, false);
    write_ts_packets(&mut out, 1, &mut cc, b"b", false, false);
    write_ts_packets(&mut out, 1, &mut cc, b"c", false, false);
    assert_eq!(cc, 1); // 14 -> 15 -> 0 -> 1
}

#[test]
fn rejects_bad_sync_byte() {
    let bad = vec![0u8; PACKET_LEN];
    assert!(parse_ts_packet(&bad).is_err());
}
