//! Unit tests for `ftyp` compatible-brand selection (sibling of `ftyp.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "unit tests may unwrap"
)]

use super::write_ftyp;
use crate::types::{Bytes, Codec, Rational, Track};

fn track(codec: Codec) -> Track {
    Track {
        id: 0,
        codec,
        time_base: Rational::new(1, 1000),
        width: 0,
        height: 0,
        extra_data: Bytes::new(),
    }
}

fn brand_bytes(tracks: &[Track]) -> Vec<u8> {
    let mut buf = Vec::new();
    write_ftyp(&mut buf, tracks);
    buf
}

#[test]
fn h264_track_gets_avc1_brand() {
    let buf = brand_bytes(&[track(Codec::H264)]);
    assert!(buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn hevc_track_gets_hvc1_brand_not_avc1() {
    let buf = brand_bytes(&[track(Codec::Hevc)]);
    assert!(buf.windows(4).any(|w| w == b"hvc1"));
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn av1_track_gets_av01_brand_not_avc1() {
    let buf = brand_bytes(&[track(Codec::Av1)]);
    assert!(buf.windows(4).any(|w| w == b"av01"));
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn vp9_track_gets_vp09_brand_not_avc1() {
    let buf = brand_bytes(&[track(Codec::Vp9)]);
    assert!(buf.windows(4).any(|w| w == b"vp09"));
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn audio_only_file_has_no_video_codec_brand() {
    let buf = brand_bytes(&[track(Codec::Aac)]);
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
    assert!(!buf.windows(4).any(|w| w == b"hvc1"));
    assert!(!buf.windows(4).any(|w| w == b"av01"));
    assert!(!buf.windows(4).any(|w| w == b"vp09"));
    // Still a well-formed ftyp: base brands present.
    assert!(buf.windows(4).any(|w| w == b"isom"));
    assert!(buf.windows(4).any(|w| w == b"mp41"));
}

#[test]
fn first_video_track_wins_when_multiple_present() {
    let buf = brand_bytes(&[track(Codec::Aac), track(Codec::Hevc), track(Codec::Vp9)]);
    assert!(buf.windows(4).any(|w| w == b"hvc1"));
    assert!(!buf.windows(4).any(|w| w == b"vp09"));
}
