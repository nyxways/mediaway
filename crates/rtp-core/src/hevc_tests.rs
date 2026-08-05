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

/// `IDR_W_RADL` VCL NAL (type=19), `LayerId=0`, `TID=1` (`TemporalId=0`).
const IDR_NAL_HEADER: [u8; 2] = [0x26, 0x01];

fn new_packetizer(max_payload_size: usize) -> Packetizer {
    Packetizer::new(max_payload_size, 97, 0x5566_7788, 2000).expect("new")
}

#[test]
fn nal_header_encode_decode_round_trips() {
    let (f, nal_type, layer_id, tid) = decode_nal_header(IDR_NAL_HEADER[0], IDR_NAL_HEADER[1]);
    assert_eq!((f, nal_type, layer_id, tid), (0, 19, 0, 1));
    assert_eq!(
        encode_nal_header(f, nal_type, layer_id, tid),
        IDR_NAL_HEADER
    );
}

#[test]
fn packetize_small_nal_produces_one_single_nal_packet() {
    let mut nal = IDR_NAL_HEADER.to_vec();
    nal.extend_from_slice(&[1, 2, 3, 4]);
    let mut pk = new_packetizer(1400);

    let packets = pk.packetize(&nal, 90_000, true).expect("packetize");
    assert_eq!(packets.len(), 1);
    assert_eq!(&packets[0].payload[..], &nal[..]);
    assert!(packets[0].header.marker);
    assert_eq!(packets[0].header.timestamp, 90_000);
    assert_eq!(packets[0].header.sequence_number, 2000);
}

#[test]
fn packetize_large_nal_fragments_into_fu_and_depacketize_reassembles() {
    let mut nal = IDR_NAL_HEADER.to_vec();
    let body: Vec<u8> = (0..5000).map(|i| (i % 256) as u8).collect();
    nal.extend_from_slice(&body);

    let mut pk = new_packetizer(200);
    let packets = pk.packetize(&nal, 90_000, true).expect("packetize");
    assert!(packets.len() > 1, "expected fragmentation into >1 packet");

    let mut expected_seq = 2000_u16;
    for (i, packet) in packets.iter().enumerate() {
        assert_eq!(packet.header.timestamp, 90_000);
        assert_eq!(packet.header.marker, i == packets.len() - 1);
        assert_eq!(packet.header.sequence_number, expected_seq);
        expected_seq = expected_seq.wrapping_add(1);
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
    let nal = [0x60u8, 1, 2, 3]; // type=48 (AP) — reserved for this crate's own FU framing
    let mut pk = new_packetizer(1400);
    let err = pk.packetize(&nal, 0, true).unwrap_err();
    assert!(matches!(err, Error::ReservedNalUnitType(48)));
}

#[test]
fn packetize_rejects_short_nal() {
    let mut pk = new_packetizer(1400);
    let err = pk.packetize(&[0x26], 0, true).unwrap_err();
    assert!(matches!(err, Error::NalUnitTooShort { needed: 2, got: 1 }));
}

#[test]
fn new_rejects_too_small_max_payload_size() {
    let err = Packetizer::new(FU_OVERHEAD, 97, 0, 0).unwrap_err();
    assert!(matches!(
        err,
        Error::MaxPayloadSizeTooSmall(n) if n == FU_OVERHEAD
    ));
}

#[test]
fn depacketize_rejects_aggregation_packet() {
    let mut depk = Depacketizer::new();
    let err = depk.depacketize(&[0x60, 0x01, 0, 1, 0xAA]).unwrap_err(); // type=48 AP
    assert!(matches!(err, Error::AggregationPacketUnsupported(48)));
}

#[test]
fn depacketize_rejects_paci_packet() {
    let mut depk = Depacketizer::new();
    let err = depk.depacketize(&[0x64, 0x01, 0, 0]).unwrap_err(); // type=50 PACI
    assert!(matches!(err, Error::PaciPacketUnsupported(50)));
}

#[test]
fn depacketize_fu_continuation_without_start_errors() {
    let mut depk = Depacketizer::new();
    // Payload header type=49 (FU), FU header S=0,E=0,FuType=19 — continuation with no prior start.
    let err = depk.depacketize(&[0x62, 0x01, 0x13, 1, 2]).unwrap_err();
    assert!(matches!(err, Error::MissingFuStart));
}

#[test]
fn depacketize_fu_unexpected_second_start_errors() {
    let mut depk = Depacketizer::new();
    depk.depacketize(&[0x62, 0x01, 0x93, 1, 2])
        .expect("first start"); // S=1
    let err = depk.depacketize(&[0x62, 0x01, 0x93, 3, 4]).unwrap_err(); // S=1 again, no E seen yet
    assert!(matches!(err, Error::UnexpectedFuStart));
}

#[test]
fn depacketize_fu_payload_too_short_errors() {
    let mut depk = Depacketizer::new();
    let err = depk.depacketize(&[0x62, 0x01]).unwrap_err(); // missing FU header byte
    assert!(matches!(
        err,
        Error::FuPayloadTooShort { needed: 3, got: 2 }
    ));
}

#[test]
fn depacketize_rejects_short_payload() {
    let mut depk = Depacketizer::new();
    let err = depk.depacketize(&[0x26]).unwrap_err();
    assert!(matches!(err, Error::NalUnitTooShort { needed: 2, got: 1 }));
}
