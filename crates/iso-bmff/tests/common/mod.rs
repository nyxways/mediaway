//! Shared helpers for iso-bmff integration / conformance tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_const_for_fn,
    clippy::print_stderr,
    unreachable_pub,
    dead_code,
    reason = "test helpers shared across integration binaries"
)]

use bytes::Bytes;
use iso_bmff::isobmff::{FourCc, parse_header};
use iso_bmff::{Codec, Demuxer, Muxer, Rational, Sample, Track};

pub const AVC_C: &[u8] = &[
    1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 4, 0x67, 0x42, 0x00, 0x1e, 1, 0, 4, 0x68, 0xce, 0x06, 0xe2,
];

pub fn h264(id: u32, extra: Bytes) -> Track {
    Track {
        id,
        codec: Codec::H264,
        time_base: Rational::new(1, 1000),
        width: 1920,
        height: 1080,
        extra_data: extra,
    }
}

pub fn aac(id: u32) -> Track {
    Track {
        id,
        codec: Codec::Aac,
        time_base: Rational::new(1, 48_000),
        width: 0,
        height: 0,
        extra_data: Bytes::from_static(&[0x11, 0x90]),
    }
}

/// One top-level ISOBMFF box in a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBox {
    pub typ: FourCc,
    pub offset: usize,
    pub size: usize,
    pub header_len: usize,
}

/// Walk top-level boxes; stops on truncated/invalid header.
#[must_use]
pub fn walk_top_boxes(buf: &[u8]) -> Vec<TopBox> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset + 8 <= buf.len() {
        let Some(h) = parse_header(&buf[offset..]) else {
            break;
        };
        if h.size == 0 || offset + h.size > buf.len() {
            break;
        }
        out.push(TopBox {
            typ: h.typ,
            offset,
            size: h.size,
            header_len: h.header_len,
        });
        offset += h.size;
    }
    out
}

/// Nested boxes inside a parent payload.
#[must_use]
pub fn walk_children(buf: &[u8], parent: TopBox) -> Vec<TopBox> {
    let start = parent.offset + parent.header_len;
    let end = parent.offset + parent.size;
    if start >= end || end > buf.len() {
        return Vec::new();
    }
    walk_top_boxes(&buf[start..end])
        .into_iter()
        .map(|mut b| {
            b.offset += start;
            b
        })
        .collect()
}

/// Mux a short H.264 fMP4 (batch 2) and return bytes.
#[must_use]
pub fn mux_tiny_h264_fmp4() -> Vec<u8> {
    let mut open = Muxer::with_fragment_batch(2);
    open.add_track(h264(0, Bytes::from_static(AVC_C)))
        .expect("track");
    let mut mux = open.begin();
    for (i, key) in [(0u64, true), (33, false)].into_iter().enumerate() {
        mux.push_packet(&Sample {
            stream_id: 0,
            pts: i64::try_from(key.0).unwrap_or(0),
            dts: i64::try_from(key.0).unwrap_or(0),
            duration: 33,
            is_keyframe: key.1,
            is_discard: false,
            payload: Bytes::from(vec![0x65 + u8::try_from(i).unwrap_or(0)]),
        })
        .expect("pkt");
    }
    mux.flush();
    let mut out = Vec::new();
    assert!(
        mux.poll_bytes(&mut out) > 0,
        "mux should produce fMP4 bytes"
    );
    out
}

/// Feed all bytes then drain packets (no panic expected).
pub fn demux_all(bytes: &[u8]) -> (usize, usize) {
    let mut d = Demuxer::new();
    for chunk in bytes.chunks(64) {
        d.push_bytes(chunk);
    }
    let streams = d.streams().len();
    let mut packets = 0;
    while d.poll_packet().is_some() {
        packets += 1;
    }
    (streams, packets)
}
