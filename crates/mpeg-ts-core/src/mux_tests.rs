//! Unit tests for the top-level MPEG-TS `Muxer`.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::Muxer;
use crate::error::Error;
use crate::packet::PACKET_LEN;
use crate::types::{ElementaryStream, StreamType};

fn one_video_stream() -> Vec<ElementaryStream> {
    vec![ElementaryStream {
        pid: 256,
        stream_type: StreamType::H264,
    }]
}

#[test]
fn new_rejects_reserved_pmt_pid() {
    assert!(matches!(
        Muxer::new(1, 0, &one_video_stream()),
        Err(Error::InvalidPid(0))
    ));
    assert!(matches!(
        Muxer::new(1, 1, &one_video_stream()),
        Err(Error::InvalidPid(1))
    ));
}

#[test]
fn new_rejects_reserved_stream_pid() {
    let streams = vec![ElementaryStream {
        pid: 1,
        stream_type: StreamType::Aac,
    }];
    assert!(Muxer::new(1, 4096, &streams).is_err());
}

#[test]
fn write_pat_pmt_produces_two_aligned_packets() {
    let mut mux = Muxer::new(1, 4096, &one_video_stream()).unwrap();
    let mut out = Vec::new();
    mux.write_pat_pmt(&mut out);
    assert_eq!(out.len(), 2 * PACKET_LEN);
    assert_eq!(out[0], 0x47);
    assert_eq!(out[PACKET_LEN], 0x47);
}

#[test]
fn write_access_unit_rejects_unregistered_pid() {
    let mut mux = Muxer::new(1, 4096, &one_video_stream()).unwrap();
    let mut out = Vec::new();
    let err = mux
        .write_access_unit(999, b"data", 0, None, false, &mut out)
        .unwrap_err();
    assert!(matches!(err, Error::UnknownPid(999)));
}

#[test]
fn write_access_unit_produces_packet_aligned_output() {
    let mut mux = Muxer::new(1, 4096, &one_video_stream()).unwrap();
    let mut out = Vec::new();
    mux.write_access_unit(256, b"keyframe payload", 90_000, None, true, &mut out)
        .unwrap();
    assert_eq!(out.len() % PACKET_LEN, 0);
    assert!(!out.is_empty());
}
