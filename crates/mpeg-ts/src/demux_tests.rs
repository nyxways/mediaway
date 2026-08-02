//! Unit tests for the top-level MPEG-TS `Demuxer`.

#![cfg(test)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::Demuxer;
use crate::mux::Muxer;
use crate::types::{ElementaryStream, StreamType};

fn streams() -> Vec<ElementaryStream> {
    vec![
        ElementaryStream {
            pid: 256,
            stream_type: StreamType::H264,
        },
        ElementaryStream {
            pid: 257,
            stream_type: StreamType::Aac,
        },
    ]
}

#[test]
fn parses_pat_pmt_and_populates_streams() {
    let mut mux = Muxer::new(1, 4096, &streams()).unwrap();
    let mut bytes = Vec::new();
    mux.write_pat_pmt(&mut bytes);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert!(demux.streams().is_empty());
    let _ = demux.poll_access_unit(); // drives packet consumption
    assert_eq!(demux.streams(), streams().as_slice());
}

#[test]
fn access_unit_completes_only_when_the_next_one_starts() {
    let mut mux = Muxer::new(1, 4096, &streams()).unwrap();
    let mut demux = Demuxer::new();

    let mut header_bytes = Vec::new();
    mux.write_pat_pmt(&mut header_bytes);
    demux.push_bytes(&header_bytes);

    let mut first_bytes = Vec::new();
    mux.write_access_unit(
        256,
        b"frame one payload",
        90_000,
        None,
        true,
        &mut first_bytes,
    )
    .unwrap();
    demux.push_bytes(&first_bytes);
    assert!(demux.poll_access_unit().unwrap().is_none()); // frame one not confirmed complete yet

    let mut second_bytes = Vec::new();
    mux.write_access_unit(256, b"frame two", 93_000, None, false, &mut second_bytes)
        .unwrap();
    demux.push_bytes(&second_bytes);

    let completed = demux
        .poll_access_unit()
        .unwrap()
        .expect("frame one now complete");
    assert_eq!(&completed.data[..], b"frame one payload");
}

#[test]
fn full_roundtrip_two_streams_multiple_access_units() {
    let mut mux = Muxer::new(1, 4096, &streams()).unwrap();
    let mut bytes = Vec::new();
    mux.write_pat_pmt(&mut bytes);
    mux.write_access_unit(256, b"video keyframe", 0, Some(0), true, &mut bytes)
        .unwrap();
    mux.write_access_unit(
        256,
        b"video interframe",
        3_000,
        Some(3_000),
        false,
        &mut bytes,
    )
    .unwrap();
    mux.write_access_unit(257, b"audio frame one", 0, None, false, &mut bytes)
        .unwrap();
    mux.write_access_unit(257, b"audio frame two", 1_024, None, false, &mut bytes)
        .unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let mut video_units = Vec::new();
    let mut audio_units = Vec::new();
    while let Some(unit) = demux.poll_access_unit().unwrap() {
        if unit.pid == 256 {
            video_units.push(unit);
        } else {
            audio_units.push(unit);
        }
    }
    // The very last access unit per PID is never confirmed complete by a
    // following PUSI within this buffer — `finish()` recovers it.
    for unit in demux.finish() {
        if unit.pid == 256 {
            video_units.push(unit);
        } else {
            audio_units.push(unit);
        }
    }

    assert_eq!(video_units.len(), 2);
    assert_eq!(&video_units[0].data[..], b"video keyframe");
    assert!(video_units[0].random_access);
    assert_eq!(video_units[0].pts_90k, 0);
    assert_eq!(video_units[0].dts_90k, Some(0));
    assert_eq!(&video_units[1].data[..], b"video interframe");
    assert!(!video_units[1].random_access);
    assert_eq!(video_units[1].pts_90k, 3_000);

    assert_eq!(audio_units.len(), 2);
    assert_eq!(&audio_units[0].data[..], b"audio frame one");
    assert_eq!(&audio_units[1].data[..], b"audio frame two");
    assert_eq!(audio_units[1].pts_90k, 1_024);
}
