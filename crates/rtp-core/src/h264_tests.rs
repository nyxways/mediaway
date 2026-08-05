#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "test-only loop indices/counters are small, bounded, and non-negative"
)]

use super::*;

/// NRI=3 (highest), type=5 (IDR slice) — an arbitrary but valid single-NAL header byte.
const IDR_NAL_HEADER: u8 = 0x65;

fn new_packetizer(max_payload_size: usize) -> Packetizer {
    Packetizer::new(max_payload_size, 96, 0x1122_3344, 1000).expect("new")
}

#[test]
fn packetize_small_nal_produces_one_single_nal_packet() {
    let mut nal = vec![IDR_NAL_HEADER];
    nal.extend_from_slice(&[1, 2, 3, 4]);
    let mut pk = new_packetizer(1400);

    let packets = pk.packetize(&nal, 90_000, true).expect("packetize");
    assert_eq!(packets.len(), 1);
    assert_eq!(&packets[0].payload[..], &nal[..]);
    assert!(packets[0].header.marker);
    assert_eq!(packets[0].header.timestamp, 90_000);
    assert_eq!(packets[0].header.sequence_number, 1000);
}

#[test]
fn packetize_large_nal_fragments_into_fu_a_and_depacketize_reassembles() {
    // Body large enough to force multiple FU-A fragments under a small MTU budget.
    let mut nal = vec![IDR_NAL_HEADER];
    let body: Vec<u8> = (0..5000).map(|i| (i % 256) as u8).collect();
    nal.extend_from_slice(&body);

    let mut pk = new_packetizer(200);
    let packets = pk.packetize(&nal, 90_000, true).expect("packetize");
    assert!(packets.len() > 1, "expected fragmentation into >1 packet");

    // Only the last packet carries the marker bit; timestamps are all equal.
    for (i, packet) in packets.iter().enumerate() {
        assert_eq!(packet.header.timestamp, 90_000);
        assert_eq!(packet.header.marker, i == packets.len() - 1);
        assert_eq!(
            packet.header.sequence_number,
            1000_u16.wrapping_add(i as u16)
        );
    }

    let mut depk = Depacketizer::new();
    let mut reassembled = None;
    for packet in &packets {
        let result = depk.depacketize(&packet.payload).expect("depacketize");
        if result.is_some() {
            reassembled = result;
        }
    }
    assert_eq!(reassembled.expect("reassembled NAL")[..], nal[..]);
}

#[test]
fn packetize_rejects_reserved_nal_type_as_input() {
    let nal = [0x38u8, 1, 2]; // type=24 (STAP-A) — reserved for this crate's own FU framing
    let mut pk = new_packetizer(1400);
    let err = pk.packetize(&nal, 0, true).unwrap_err();
    assert!(matches!(err, Error::ReservedNalUnitType(24)));
}

#[test]
fn packetize_rejects_empty_nal() {
    let mut pk = new_packetizer(1400);
    let err = pk.packetize(&[], 0, true).unwrap_err();
    assert!(matches!(err, Error::NalUnitTooShort { needed: 1, got: 0 }));
}

#[test]
fn new_rejects_too_small_max_payload_size() {
    let err = Packetizer::new(FU_A_OVERHEAD, 96, 0, 0).unwrap_err();
    assert!(matches!(
        err,
        Error::MaxPayloadSizeTooSmall(n) if n == FU_A_OVERHEAD
    ));
}

#[test]
fn depacketize_rejects_aggregation_packet() {
    let mut depk = Depacketizer::new();
    let err = depk.depacketize(&[0x38, 0, 1, 0xAA]).unwrap_err(); // type=24 STAP-A
    assert!(matches!(err, Error::AggregationPacketUnsupported(24)));
}

#[test]
fn depacketize_rejects_interleaved_fu_b() {
    let mut depk = Depacketizer::new();
    let err = depk.depacketize(&[0x7D, 0x85, 0, 0]).unwrap_err(); // type=29 FU-B
    assert!(matches!(err, Error::InterleavedFragmentUnsupported(29)));
}

#[test]
fn depacketize_rejects_reserved_type() {
    let mut depk = Depacketizer::new();
    let err = depk.depacketize(&[0x60]).unwrap_err(); // type=0, reserved
    assert!(matches!(err, Error::UnsupportedNalUnitType(0)));
}

#[test]
fn depacketize_fu_a_continuation_without_start_errors() {
    let mut depk = Depacketizer::new();
    // FU indicator type=28, FU header S=0,E=0,type=5 — a continuation with no prior start.
    let err = depk.depacketize(&[0x7C, 0x05, 1, 2]).unwrap_err();
    assert!(matches!(err, Error::MissingFuStart));
}

#[test]
fn depacketize_fu_a_unexpected_second_start_errors() {
    let mut depk = Depacketizer::new();
    depk.depacketize(&[0x7C, 0x85, 1, 2]).expect("first start"); // S=1
    let err = depk.depacketize(&[0x7C, 0x85, 3, 4]).unwrap_err(); // S=1 again, no E seen yet
    assert!(matches!(err, Error::UnexpectedFuStart));
}

#[test]
fn depacketize_fu_a_payload_too_short_errors() {
    let mut depk = Depacketizer::new();
    let err = depk.depacketize(&[0x7C]).unwrap_err(); // missing FU header byte
    assert!(matches!(
        err,
        Error::FuPayloadTooShort { needed: 2, got: 1 }
    ));
}
