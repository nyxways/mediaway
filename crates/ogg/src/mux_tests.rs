//! Unit tests for Ogg page mux.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::Muxer;
use crate::error::Error;

#[test]
fn push_packet_writes_capture_pattern_and_flags() {
    let mut mux = Muxer::new(42);
    let mut out = Vec::new();
    mux.push_packet(b"hello", 0, false, &mut out).unwrap();

    assert_eq!(&out[0..4], b"OggS");
    assert_eq!(out[4], 0); // version
    assert_eq!(out[5] & 0x02, 0x02); // bos set on first page
    assert_eq!(out[5] & 0x04, 0); // eos not set
}

#[test]
fn first_page_sets_bos_only_once() {
    let mut mux = Muxer::new(1);
    let mut out = Vec::new();
    mux.push_packet(b"a", 0, false, &mut out).unwrap();
    mux.push_packet(b"b", 1, true, &mut out).unwrap();

    assert_eq!(out[5] & 0x02, 0x02); // first page: bos

    // Second page starts right after the first page's fixed header + 1-byte
    // segment table + 1-byte payload = 27 + 1 + 1 = 29 bytes in.
    let second_page_start = 29;
    assert_eq!(&out[second_page_start..second_page_start + 4], b"OggS");
    assert_eq!(out[second_page_start + 5] & 0x02, 0); // not bos
    assert_eq!(out[second_page_start + 5] & 0x04, 0x04); // eos
}

#[test]
fn rejects_packet_larger_than_single_page() {
    let mut mux = Muxer::new(1);
    let mut out = Vec::new();
    let huge = vec![0u8; 65_025];
    let err = mux.push_packet(&huge, 0, false, &mut out).unwrap_err();
    assert!(matches!(err, Error::PacketTooLargeForSinglePage(65_025)));
}

#[test]
fn sequence_number_increments() {
    let mut mux = Muxer::new(7);
    let mut out = Vec::new();
    mux.push_packet(b"x", 0, false, &mut out).unwrap();
    mux.push_packet(b"y", 1, false, &mut out).unwrap();

    let seq_at = |page_start: usize| {
        u32::from_le_bytes(out[page_start + 18..page_start + 22].try_into().unwrap())
    };
    assert_eq!(seq_at(0), 0);
    let second_page_start = 27 + 1 + 1; // header + 1 segment + 1-byte payload
    assert_eq!(seq_at(second_page_start), 1);
}
