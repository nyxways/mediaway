//! Integration: public API packetize → wire bytes → parse → depacketize round
//! trip, for both H.264 and HEVC, including NAL units larger than the MTU
//! budget (forcing FU-A/FU fragmentation).

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests may unwrap"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "test-only loop index is small, bounded, and non-negative"
)]

use rtp_core::{RtpPacket, h264, hevc};

/// Serialize every packet to bytes and parse it back, simulating what a real
/// caller's socket send/receive would do — this is what actually exercises
/// [`RtpPacket::write`]/[`RtpPacket::parse`], not just the in-memory `Vec<RtpPacket>`.
fn send_and_receive(packets: &[RtpPacket]) -> Vec<RtpPacket> {
    packets
        .iter()
        .map(|packet| {
            let mut wire = Vec::new();
            packet.write(&mut wire).expect("write");
            RtpPacket::parse(&wire).expect("parse")
        })
        .collect()
}

#[test]
fn h264_small_nal_round_trips_through_wire_bytes() {
    let mut nal = vec![0x67u8]; // SPS NAL header (type 7)
    nal.extend_from_slice(&[0xAA; 32]);

    let mut pk = h264::Packetizer::new(1400, 96, 0xC0FF_EE01, 1).expect("new packetizer");
    let packets = pk.packetize(&nal, 90_000, true).expect("packetize");
    assert_eq!(packets.len(), 1);

    let received = send_and_receive(&packets);
    let mut depk = h264::Depacketizer::new();
    let nal_out = depk
        .depacketize(&received[0].payload)
        .expect("depacketize")
        .expect("complete NAL");
    assert_eq!(&nal_out[..], &nal[..]);
    assert!(received[0].header.marker);
}

#[test]
fn h264_large_nal_fragments_and_reassembles_through_wire_bytes() {
    let mut nal = vec![0x65u8]; // IDR slice NAL header (type 5)
    let body: Vec<u8> = (0..3000).map(|i| (i % 251) as u8).collect();
    nal.extend_from_slice(&body);

    let mut pk = h264::Packetizer::new(300, 96, 0xC0FF_EE02, 100).expect("new packetizer");
    let packets = pk.packetize(&nal, 45_000, true).expect("packetize");
    assert!(packets.len() > 1);

    let received = send_and_receive(&packets);
    let mut depk = h264::Depacketizer::new();
    let mut nal_out = None;
    for packet in &received {
        assert_eq!(packet.header.timestamp, 45_000);
        if let Some(nal) = depk.depacketize(&packet.payload).expect("depacketize") {
            nal_out = Some(nal);
        }
    }
    assert_eq!(&nal_out.expect("complete NAL")[..], &nal[..]);
}

#[test]
fn hevc_small_nal_round_trips_through_wire_bytes() {
    let mut nal = vec![0x40u8, 0x01]; // VPS NAL header (type 32), LayerId=0, TID=1
    nal.extend_from_slice(&[0xBB; 32]);

    let mut pk = hevc::Packetizer::new(1400, 98, 0xFEED_0001, 1).expect("new packetizer");
    let packets = pk.packetize(&nal, 90_000, true).expect("packetize");
    assert_eq!(packets.len(), 1);

    let received = send_and_receive(&packets);
    let mut depk = hevc::Depacketizer::new();
    let nal_out = depk
        .depacketize(&received[0].payload)
        .expect("depacketize")
        .expect("complete NAL");
    assert_eq!(&nal_out[..], &nal[..]);
    assert!(received[0].header.marker);
}

#[test]
fn hevc_large_nal_fragments_and_reassembles_through_wire_bytes() {
    let mut nal = vec![0x26u8, 0x01]; // IDR_W_RADL NAL header (type 19), LayerId=0, TID=1
    let body: Vec<u8> = (0..3000).map(|i| (i % 251) as u8).collect();
    nal.extend_from_slice(&body);

    let mut pk = hevc::Packetizer::new(300, 98, 0xFEED_0002, 200).expect("new packetizer");
    let packets = pk.packetize(&nal, 45_000, true).expect("packetize");
    assert!(packets.len() > 1);

    let received = send_and_receive(&packets);
    let mut depk = hevc::Depacketizer::new();
    let mut nal_out = None;
    for packet in &received {
        assert_eq!(packet.header.timestamp, 45_000);
        if let Some(nal) = depk.depacketize(&packet.payload).expect("depacketize") {
            nal_out = Some(nal);
        }
    }
    assert_eq!(&nal_out.expect("complete NAL")[..], &nal[..]);
}

#[test]
fn only_last_packet_of_an_access_unit_carries_the_marker_bit() {
    // Two NAL units in one access unit: non-last has marker=false, last has marker=true.
    let mut pk = h264::Packetizer::new(1400, 96, 0x1234_5678, 1).expect("new packetizer");
    let non_last = pk.packetize(&[0x67, 1, 2], 90_000, false).expect("nal 1");
    let last = pk.packetize(&[0x65, 3, 4], 90_000, true).expect("nal 2");

    assert!(non_last.iter().all(|p| !p.header.marker));
    assert!(last.iter().all(|p| p.header.marker));
}
