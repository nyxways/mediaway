//! Unit tests for Ogg page demux — including multi-packet pages and
//! cross-page packet continuation, which this crate's own `Muxer` never
//! produces (it only ever emits one packet per page) but any real Ogg encoder
//! does; the demuxer must handle the general case regardless.

#![cfg(test)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::Demuxer;
use crate::crc::crc32_ogg;
use crate::mux::Muxer;

/// Hand-build one raw Ogg page (bypassing `Muxer`, which only ever emits one
/// packet per page) so tests can exercise multi-packet and continuation pages.
fn build_page(
    serial: u32,
    sequence: u32,
    granule: i64,
    continued: bool,
    bos: bool,
    eos: bool,
    segment_table: &[u8],
    payload: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"OggS");
    out.push(0);
    let flags = u8::from(continued) | (u8::from(bos) << 1) | (u8::from(eos) << 2);
    out.push(flags);
    out.extend_from_slice(&granule.to_le_bytes());
    out.extend_from_slice(&serial.to_le_bytes());
    out.extend_from_slice(&sequence.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(u8::try_from(segment_table.len()).expect("segment table fits u8"));
    out.extend_from_slice(segment_table);
    out.extend_from_slice(payload);

    let crc = crc32_ogg(&out);
    out[22..26].copy_from_slice(&crc.to_le_bytes());
    out
}

#[test]
fn roundtrips_single_packet_via_muxer() {
    let mut mux = Muxer::new(1);
    let mut bytes = Vec::new();
    mux.push_packet(b"payload", 100, true, &mut bytes).unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let packet = demux.poll_packet().unwrap().expect("packet");
    assert_eq!(&packet.data[..], b"payload");
    assert_eq!(packet.granule_position, 100);
    assert_eq!(packet.serial, 1);
    assert!(packet.bos);
    assert!(packet.eos);
    assert!(demux.poll_packet().unwrap().is_none());
}

#[test]
fn parses_multiple_packets_from_one_page() {
    // Two packets "ab" (len 2, terminator 2) and "cde" (len 3, terminator 3),
    // packed into one page's segment table: [2, 3].
    let page = build_page(9, 0, 0, false, true, false, &[2, 3], b"abcde");

    let mut demux = Demuxer::new();
    demux.push_bytes(&page);
    let first = demux.poll_packet().unwrap().expect("first");
    assert_eq!(&first.data[..], b"ab");
    let second = demux.poll_packet().unwrap().expect("second");
    assert_eq!(&second.data[..], b"cde");
    assert!(demux.poll_packet().unwrap().is_none());
}

#[test]
fn reassembles_packet_spanning_two_pages() {
    // Page 1: one 255-byte segment run with no terminator -> packet continues.
    let part1 = vec![0xAAu8; 255];
    let page1 = build_page(2, 0, 0, false, true, false, &[255], &part1);
    // Page 2: continuation flag set; segment table [10] completes the packet
    // with 10 more bytes.
    let part2 = vec![0xBBu8; 10];
    let page2 = build_page(2, 1, 5, true, false, true, &[10], &part2);

    let mut demux = Demuxer::new();
    demux.push_bytes(&page1);
    assert!(demux.poll_packet().unwrap().is_none()); // no complete packet yet

    demux.push_bytes(&page2);
    let packet = demux.poll_packet().unwrap().expect("reassembled packet");
    assert_eq!(packet.data.len(), 265);
    assert_eq!(&packet.data[..255], &part1[..]);
    assert_eq!(&packet.data[255..], &part2[..]);
    assert_eq!(packet.granule_position, 5); // page 2's granule position
}

#[test]
fn waits_for_more_bytes_on_partial_page() {
    let mut mux = Muxer::new(1);
    let mut bytes = Vec::new();
    mux.push_packet(b"hello world", 0, false, &mut bytes)
        .unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes[..bytes.len() - 3]);
    assert!(demux.poll_packet().unwrap().is_none());

    demux.push_bytes(&bytes[bytes.len() - 3..]);
    assert!(demux.poll_packet().unwrap().is_some());
}

#[test]
fn rejects_bad_capture_pattern() {
    let mut demux = Demuxer::new();
    demux.push_bytes(&[0u8; 27]);
    assert!(demux.poll_packet().is_err());
}

#[test]
fn rejects_corrupted_crc() {
    let mut mux = Muxer::new(1);
    let mut bytes = Vec::new();
    mux.push_packet(b"payload", 0, false, &mut bytes).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF; // corrupt the payload without touching the CRC field

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert!(demux.poll_packet().is_err());
}

#[test]
fn rejects_continuation_flag_mismatch() {
    // A page claiming to continue a packet when nothing preceded it.
    let page = build_page(1, 0, 0, true, false, false, &[3], b"abc");
    let mut demux = Demuxer::new();
    demux.push_bytes(&page);
    assert!(demux.poll_packet().is_err());
}
