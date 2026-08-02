//! Unit tests for MPEG audio (Layer III) mux.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::Muxer;
use crate::types::{ChannelMode, FrameHeader, MpegVersion};

fn mpeg1_128k_44k1_stereo() -> FrameHeader {
    FrameHeader {
        version: MpegVersion::Mpeg1,
        bitrate_kbps: 128,
        sample_rate: 44_100,
        channel_mode: ChannelMode::Stereo,
    }
}

#[test]
fn new_rejects_non_standard_bitrate() {
    let mut header = mpeg1_128k_44k1_stereo();
    header.bitrate_kbps = 129;
    assert!(Muxer::new(header).is_err());
}

#[test]
fn new_rejects_non_standard_sample_rate() {
    let mut header = mpeg1_128k_44k1_stereo();
    header.sample_rate = 44_000;
    assert!(Muxer::new(header).is_err());
}

#[test]
fn frame_len_matches_known_reference_value() {
    // MPEG-1 Layer III, 128 kbps, 44100 Hz, no padding: well-known value 417.
    assert_eq!(mpeg1_128k_44k1_stereo().frame_len(false), 417);
    assert_eq!(mpeg1_128k_44k1_stereo().frame_len(true), 418);
}

#[test]
fn write_frame_rejects_wrong_body_length() {
    let mux = Muxer::new(mpeg1_128k_44k1_stereo()).unwrap();
    let mut out = Vec::new();
    let wrong_body = vec![0u8; 100];
    assert!(mux.write_frame(&wrong_body, false, &mut out).is_err());
}

#[test]
fn write_frame_produces_valid_sync() {
    let mux = Muxer::new(mpeg1_128k_44k1_stereo()).unwrap();
    let mut out = Vec::new();
    let body = vec![0xAB; 413]; // 417 - 4
    mux.write_frame(&body, false, &mut out).unwrap();

    assert_eq!(out.len(), 417);
    assert_eq!(out[0], 0xFF);
    assert_eq!(out[1] & 0xE0, 0xE0);
    assert_eq!((out[1] >> 3) & 0x03, 0b11); // MPEG-1
    assert_eq!((out[1] >> 1) & 0x03, 0b01); // Layer III
}
