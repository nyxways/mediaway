//! Unit tests for the ADTS facade adapter (sibling of `adts.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn mux_then_demux_roundtrips_frame_and_synthesizes_timing() {
    let mut mux = Muxer::new(44_100, 2).expect("valid sample rate");
    let raw_aac = [0xAB; 100];

    let mut bytes = Vec::new();
    mux.push_packet(&Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: mediaway_common::Bytes::copy_from_slice(&raw_aac),
    })
    .expect("push");
    mux.poll_bytes(&mut bytes);
    assert!(!bytes.is_empty());

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let p1 = demux.poll_packet().expect("frame 1");
    assert_eq!(&p1.payload[..], &raw_aac[..]);
    assert_eq!(p1.pts, 0);
    assert_eq!(p1.duration, 1024);

    assert_eq!(demux.streams().len(), 1);
    assert_eq!(demux.streams()[0].codec(), CodecKind::Aac);
    assert_eq!(demux.streams()[0].sample_rate(), Some(44_100));
    assert_eq!(demux.streams()[0].channels(), Some(2));
}

#[test]
fn second_frame_advances_pts_by_frame_size() {
    let mut mux = Muxer::new(48_000, 1).expect("valid sample rate");
    let raw_aac = [0x11; 10];
    let mut bytes = Vec::new();
    mux.push_packet(&Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: mediaway_common::Bytes::copy_from_slice(&raw_aac),
    })
    .expect("push 1");
    mux.push_packet(&Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: mediaway_common::Bytes::copy_from_slice(&raw_aac),
    })
    .expect("push 2");
    mux.poll_bytes(&mut bytes);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let p1 = demux.poll_packet().expect("frame 1");
    let p2 = demux.poll_packet().expect("frame 2");
    assert_eq!(p1.pts, 0);
    assert_eq!(p2.pts, 1024);
}

#[test]
fn unsupported_sample_rate_is_rejected() {
    assert!(matches!(
        Muxer::new(12_345, 2),
        Err(Error::UnsupportedSampleRate(12_345))
    ));
}
