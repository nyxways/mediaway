//! Unit tests for FLV mux.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::Muxer;
use crate::error::Error;
use crate::types::{Tag, TagType};
use bytes::Bytes;

#[test]
fn write_header_produces_flv_signature_and_flags() {
    let mut mux = Muxer::new();
    let mut out = Vec::new();
    mux.write_header(true, true, &mut out);

    assert_eq!(&out[0..3], b"FLV");
    assert_eq!(out[3], 1); // version
    assert_eq!(out[4], 0x05); // audio(0x04) | video(0x01)
    assert_eq!(u32::from_be_bytes(out[5..9].try_into().unwrap()), 9);
    assert_eq!(u32::from_be_bytes(out[9..13].try_into().unwrap()), 0); // PreviousTagSize0
}

#[test]
fn write_tag_before_header_errors() {
    let mux = Muxer::new();
    let mut out = Vec::new();
    let tag = Tag {
        tag_type: TagType::Audio,
        timestamp_ms: 0,
        data: Bytes::from_static(&[1, 2, 3]),
    };
    assert!(matches!(
        mux.write_tag(&tag, &mut out),
        Err(Error::HeaderNotWritten)
    ));
}

#[test]
fn write_tag_appends_header_data_and_trailer() {
    let mut mux = Muxer::new();
    let mut out = Vec::new();
    mux.write_header(true, false, &mut out);
    let header_len = out.len();

    let tag = Tag {
        tag_type: TagType::Video,
        timestamp_ms: 0x0102_0304,
        data: Bytes::from_static(&[0xAA, 0xBB, 0xCC]),
    };
    mux.write_tag(&tag, &mut out).unwrap();

    let tag_bytes = &out[header_len..];
    assert_eq!(tag_bytes[0], 9); // TagType::Video
    let data_size = (usize::from(tag_bytes[1]) << 16)
        | (usize::from(tag_bytes[2]) << 8)
        | usize::from(tag_bytes[3]);
    assert_eq!(data_size, 3);
    // Timestamp: lower 24 bits then extended byte.
    assert_eq!(tag_bytes[4], 0x02);
    assert_eq!(tag_bytes[5], 0x03);
    assert_eq!(tag_bytes[6], 0x04);
    assert_eq!(tag_bytes[7], 0x01); // extended (top 8 bits)
    assert_eq!(&tag_bytes[11..14], &[0xAA, 0xBB, 0xCC]);
    let trailer = u32::from_be_bytes(tag_bytes[14..18].try_into().unwrap());
    assert_eq!(trailer, 14); // 11-byte header + 3 bytes data
}
