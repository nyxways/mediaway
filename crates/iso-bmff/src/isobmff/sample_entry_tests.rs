//! Unit tests for `vp09`/`vpcC` and `avc1`/`avcC` sample-entry write/parse.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "unit tests"
)]

use super::{parse_sample_entry, write_stsd};
use crate::isobmff::parse_header;
use crate::types::{Bytes, Codec, Rational, Track};

fn vp9_track(extra: Bytes) -> Track {
    Track {
        id: 0,
        codec: Codec::Vp9,
        time_base: Rational::new(1, 1000),
        width: 640,
        height: 480,
        extra_data: extra,
    }
}

fn h264_track() -> Track {
    Track {
        id: 0,
        codec: Codec::H264,
        time_base: Rational::new(1, 1000),
        width: 320,
        height: 240,
        extra_data: Bytes::new(),
    }
}

fn hevc_track(extra: Bytes) -> Track {
    Track {
        id: 0,
        codec: Codec::Hevc,
        time_base: Rational::new(1, 1000),
        width: 1920,
        height: 1080,
        extra_data: extra,
    }
}

fn av1_track(extra: Bytes) -> Track {
    Track {
        id: 0,
        codec: Codec::Av1,
        time_base: Rational::new(1, 1000),
        width: 3840,
        height: 2160,
        extra_data: extra,
    }
}

fn parse_stsd_entry(stsd: &[u8]) -> (u32, u32, Codec, Bytes) {
    let hdr = parse_header(stsd).expect("stsd header");
    let body = &stsd[hdr.header_len..hdr.size];
    let mut width = 0;
    let mut height = 0;
    let mut codec = Codec::H264;
    let mut extra = Bytes::new();
    let mut encryption = None;
    parse_sample_entry(
        &body[8..],
        &mut width,
        &mut height,
        &mut codec,
        &mut extra,
        &mut encryption,
    );
    assert!(encryption.is_none());
    (width, height, codec, extra)
}

#[test]
fn vp9_sample_entry_writes_vp09_not_avc1() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &vp9_track(Bytes::new()));
    assert!(buf.windows(4).any(|w| w == b"vp09"));
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn h264_sample_entry_still_writes_avc1() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &h264_track());
    assert!(buf.windows(4).any(|w| w == b"avc1"));
    assert!(!buf.windows(4).any(|w| w == b"vp09"));
}

#[test]
fn vp9_sample_entry_roundtrips_dimensions_and_codec() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &vp9_track(Bytes::new()));
    let (width, height, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Vp9);
    assert_eq!(width, 640);
    assert_eq!(height, 480);
    assert!(!extra.is_empty(), "placeholder vpcC should round-trip");
}

#[test]
fn vp9_sample_entry_reuses_demuxed_vpcc_payload() {
    let demuxed_vpcc = Bytes::from_static(&[1, 0, 0, 0, 2, 62, 0x8a, 1, 1, 1, 0, 0]);
    let mut buf = Vec::new();
    write_stsd(&mut buf, &vp9_track(demuxed_vpcc.clone()));
    let (_, _, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Vp9);
    assert_eq!(extra, demuxed_vpcc);
}

#[test]
fn hevc_sample_entry_writes_hvc1_not_avc1() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &hevc_track(Bytes::new()));
    assert!(buf.windows(4).any(|w| w == b"hvc1"));
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn hevc_sample_entry_roundtrips_dimensions_and_codec() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &hevc_track(Bytes::new()));
    let (width, height, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Hevc);
    assert_eq!(width, 1920);
    assert_eq!(height, 1080);
    assert!(!extra.is_empty(), "placeholder hvcC should round-trip");
}

#[test]
fn hevc_sample_entry_reuses_demuxed_hvcc_payload() {
    let demuxed_hvcc = Bytes::from_static(&[
        1, 1, 0x60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x5A, 0xF0, 0, 0xFC, 0xFD, 0xF8, 0xF8, 0, 0, 0x03, 0,
    ]);
    let mut buf = Vec::new();
    write_stsd(&mut buf, &hevc_track(demuxed_hvcc.clone()));
    let (_, _, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Hevc);
    assert_eq!(extra, demuxed_hvcc);
}

#[test]
fn av1_sample_entry_writes_av01_not_avc1() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &av1_track(Bytes::new()));
    assert!(buf.windows(4).any(|w| w == b"av01"));
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn av1_sample_entry_roundtrips_dimensions_and_codec() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &av1_track(Bytes::new()));
    let (width, height, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Av1);
    assert_eq!(width, 3840);
    assert_eq!(height, 2160);
    assert!(!extra.is_empty(), "placeholder av1C should round-trip");
}

#[test]
fn av1_sample_entry_reuses_demuxed_av1c_payload() {
    let demuxed_av1c = Bytes::from_static(&[0x81, 0x08, 0x0C, 0]);
    let mut buf = Vec::new();
    write_stsd(&mut buf, &av1_track(demuxed_av1c.clone()));
    let (_, _, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Av1);
    assert_eq!(extra, demuxed_av1c);
}
