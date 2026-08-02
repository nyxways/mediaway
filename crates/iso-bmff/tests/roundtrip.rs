//! Integration: typestate mux ↔ demux fMP4 (H.264 + AAC).

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_const_for_fn,
    reason = "integration tests may unwrap"
)]

use bytes::Bytes;
use iso_bmff::{Codec, Demuxer, Muxer, Rational, Sample, Track};

fn h264(id: u32, extra: Bytes) -> Track {
    Track {
        id,
        codec: Codec::H264,
        time_base: Rational::new(1, 1000),
        width: 1920,
        height: 1080,
        extra_data: extra,
    }
}

fn vp9(id: u32, extra: Bytes) -> Track {
    Track {
        id,
        codec: Codec::Vp9,
        time_base: Rational::new(1, 1000),
        width: 640,
        height: 480,
        extra_data: extra,
    }
}

fn hevc(id: u32, extra: Bytes) -> Track {
    Track {
        id,
        codec: Codec::Hevc,
        time_base: Rational::new(1, 1000),
        width: 1920,
        height: 1080,
        extra_data: extra,
    }
}

fn av1(id: u32, extra: Bytes) -> Track {
    Track {
        id,
        codec: Codec::Av1,
        time_base: Rational::new(1, 1000),
        width: 3840,
        height: 2160,
        extra_data: extra,
    }
}

fn aac(id: u32) -> Track {
    Track {
        id,
        codec: Codec::Aac,
        time_base: Rational::new(1, 48_000),
        width: 0,
        height: 0,
        extra_data: Bytes::from_static(&[0x11, 0x90]),
    }
}

const AVC_C: &[u8] = &[
    1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 4, 0x67, 0x42, 0x00, 0x1e, 1, 0, 4, 0x68, 0xce, 0x06, 0xe2,
];

#[test]
fn fmp4_h264_roundtrip() {
    let mut open = Muxer::with_fragment_batch(2);
    open.add_track(h264(0, Bytes::from_static(AVC_C)))
        .expect("track");
    let mut mux = open.begin();

    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 33,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(&[0, 0, 0, 2, 0x65, 0x88]),
    })
    .expect("p1");
    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 33,
        dts: 33,
        duration: 33,
        is_keyframe: false,
        is_discard: false,
        payload: Bytes::from_static(&[0, 0, 0, 2, 0x41, 0x99]),
    })
    .expect("p2");
    mux.flush();

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    assert_eq!(&bytes[4..8], b"ftyp");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.streams().len(), 1);
    assert_eq!(demux.streams()[0].codec, Codec::H264);
    assert_eq!(demux.streams()[0].width, 1920);
    assert!(!demux.streams()[0].extra_data.is_empty());

    let a = demux.poll_packet().expect("a");
    assert!(a.is_keyframe);
    assert_eq!(&a.payload[..], &[0, 0, 0, 2, 0x65, 0x88]);
    let b = demux.poll_packet().expect("b");
    assert!(!b.is_keyframe);
    assert_eq!(b.pts, 33);
}

#[test]
fn fmp4_vp9_roundtrip() {
    let mut open = Muxer::with_fragment_batch(1);
    open.add_track(vp9(0, Bytes::new())).expect("track");
    let mut mux = open.begin();

    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 33,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(&[0x82, 0x49, 0x83, 0x42]),
    })
    .expect("p1");
    mux.flush();

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    assert!(bytes.windows(4).any(|w| w == b"vp09"));

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.streams().len(), 1);
    assert_eq!(demux.streams()[0].codec, Codec::Vp9);
    assert_eq!(demux.streams()[0].width, 640);
    assert_eq!(demux.streams()[0].height, 480);

    let a = demux.poll_packet().expect("a");
    assert!(a.is_keyframe);
    assert_eq!(&a.payload[..], &[0x82, 0x49, 0x83, 0x42]);
}

#[test]
fn fmp4_hevc_roundtrip() {
    let mut open = Muxer::with_fragment_batch(1);
    open.add_track(hevc(0, Bytes::new())).expect("track");
    let mut mux = open.begin();

    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 33,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(&[0, 0, 0, 2, 0x26, 0x01]),
    })
    .expect("p1");
    mux.flush();

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    assert!(bytes.windows(4).any(|w| w == b"hvc1"));
    assert!(!bytes.windows(4).any(|w| w == b"avc1"));

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.streams().len(), 1);
    assert_eq!(demux.streams()[0].codec, Codec::Hevc);
    assert_eq!(demux.streams()[0].width, 1920);
    assert_eq!(demux.streams()[0].height, 1080);
    assert!(!demux.streams()[0].extra_data.is_empty());

    let a = demux.poll_packet().expect("a");
    assert!(a.is_keyframe);
    assert_eq!(&a.payload[..], &[0, 0, 0, 2, 0x26, 0x01]);
}

#[test]
fn fmp4_av1_roundtrip() {
    let mut open = Muxer::with_fragment_batch(1);
    open.add_track(av1(0, Bytes::new())).expect("track");
    let mut mux = open.begin();

    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 33,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(&[0x12, 0x00, 0x0A, 0x0B]),
    })
    .expect("p1");
    mux.flush();

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    assert!(bytes.windows(4).any(|w| w == b"av01"));
    assert!(!bytes.windows(4).any(|w| w == b"avc1"));

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.streams().len(), 1);
    assert_eq!(demux.streams()[0].codec, Codec::Av1);
    assert_eq!(demux.streams()[0].width, 3840);
    assert_eq!(demux.streams()[0].height, 2160);
    assert!(!demux.streams()[0].extra_data.is_empty());

    let a = demux.poll_packet().expect("a");
    assert!(a.is_keyframe);
    assert_eq!(&a.payload[..], &[0x12, 0x00, 0x0A, 0x0B]);
}

#[test]
fn fmp4_av_and_annex_b() {
    let mut open = Muxer::with_fragment_batch(1);
    open.add_track(h264(0, Bytes::new())).expect("v");
    open.add_track(aac(1)).expect("a");
    let mut mux = open.begin();

    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 33,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from(vec![
            0, 0, 0, 1, 0x67, 0x42, 0x00, 0x1e, 0xaa, 0, 0, 0, 1, 0x68, 0xce, 0x06, 0xe2, 0, 0, 0,
            1, 0x65, 0x88, 0x84,
        ]),
    })
    .expect("annex");
    mux.push_packet(&Sample {
        stream_id: 1,
        pts: 0,
        dts: 0,
        duration: 1024,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(&[0x21, 0x10, 0x04, 0x60]),
    })
    .expect("aac");
    mux.flush();
    assert!(!mux.tracks()[0].extra_data.is_empty());

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    assert!(bytes.windows(4).any(|w| w == b"soun"));
    assert!(bytes.windows(4).any(|w| w == b"smhd"));

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.streams().len(), 2);
    let audio = demux.streams().iter().find(|s| s.id == 1).expect("a");
    assert_eq!(audio.codec, Codec::Aac);
    assert!(!audio.extra_data.is_empty());

    let mut got_v = false;
    let mut got_a = false;
    while let Some(p) = demux.poll_packet() {
        if p.stream_id == 0 {
            got_v = true;
            assert_eq!(&p.payload[..4], &5u32.to_be_bytes());
        }
        if p.stream_id == 1 {
            got_a = true;
        }
    }
    assert!(got_v && got_a);
}
