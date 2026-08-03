//! Unit tests for FLV demux.

#![cfg(test)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::Demuxer;
use crate::mux::Muxer;
use crate::types::{Tag, TagType};
use bytes::Bytes;

#[test]
fn parses_header_flags() {
    let mut mux = Muxer::new();
    let mut bytes = Vec::new();
    mux.write_header(true, false, &mut bytes);

    let mut demux = Demuxer::new();
    assert_eq!(demux.has_audio(), None);
    demux.push_bytes(&bytes);
    // Force header parsing by attempting a (failing, out-of-data) tag poll.
    let _ = demux.poll_tag();
    assert_eq!(demux.has_audio(), Some(true));
    assert_eq!(demux.has_video(), Some(false));
}

#[test]
fn roundtrips_single_tag_via_muxer() {
    let mut mux = Muxer::new();
    let mut bytes = Vec::new();
    mux.write_header(true, true, &mut bytes);
    let tag = Tag {
        tag_type: TagType::Audio,
        timestamp_ms: 12_345,
        data: Bytes::from_static(&[1, 2, 3, 4, 5]),
    };
    mux.write_tag(&tag, &mut bytes).unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let got = demux.poll_tag().unwrap().expect("tag");
    assert_eq!(got, tag);
    assert!(demux.poll_tag().unwrap().is_none());
}

#[test]
fn roundtrips_multiple_tags() {
    let mut mux = Muxer::new();
    let mut bytes = Vec::new();
    mux.write_header(true, true, &mut bytes);
    let tags = [
        Tag {
            tag_type: TagType::Video,
            timestamp_ms: 0,
            data: Bytes::from_static(&[9, 9]),
        },
        Tag {
            tag_type: TagType::Audio,
            timestamp_ms: 33,
            data: Bytes::from_static(&[1]),
        },
        Tag {
            tag_type: TagType::ScriptData,
            timestamp_ms: 0,
            data: Bytes::from_static(&[0xAA, 0xBB, 0xCC, 0xDD]),
        },
    ];
    for tag in &tags {
        mux.write_tag(tag, &mut bytes).unwrap();
    }

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    for tag in &tags {
        let got = demux.poll_tag().unwrap().expect("tag");
        assert_eq!(&got, tag);
    }
    assert!(demux.poll_tag().unwrap().is_none());
}

#[test]
fn waits_for_more_bytes_across_header_and_tag_boundaries() {
    let mut mux = Muxer::new();
    let mut bytes = Vec::new();
    mux.write_header(true, true, &mut bytes);
    let tag = Tag {
        tag_type: TagType::Audio,
        timestamp_ms: 1,
        data: Bytes::from_static(&[7, 7, 7]),
    };
    mux.write_tag(&tag, &mut bytes).unwrap();

    let mut demux = Demuxer::new();
    // Feed one byte at a time; must never error, only eventually yield the tag.
    let mut got = None;
    for i in 0..bytes.len() {
        demux.push_bytes(&bytes[i..=i]);
        if let Some(t) = demux.poll_tag().unwrap() {
            got = Some(t);
            break;
        }
    }
    assert_eq!(got, Some(tag));
}

#[test]
fn rejects_bad_signature() {
    let mut demux = Demuxer::new();
    demux.push_bytes(&[0u8; 13]);
    assert!(demux.poll_tag().is_err());
}

#[test]
fn rejects_unknown_tag_type() {
    let mut mux = Muxer::new();
    let mut bytes = Vec::new();
    mux.write_header(false, false, &mut bytes);
    bytes.extend_from_slice(&[99, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // bogus tag type 99, 0-length data

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert!(demux.poll_tag().is_err());
}
