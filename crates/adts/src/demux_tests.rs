//! Unit tests for ADTS demux.

#![cfg(test)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::Demuxer;
use crate::mux::Muxer;
use crate::types::{AacProfile, AdtsConfig};
use bytes::Bytes;

fn lc_stereo_48k() -> AdtsConfig {
    AdtsConfig {
        profile: AacProfile::Lc,
        sample_rate: 48_000,
        channels: 2,
    }
}

#[test]
fn roundtrips_single_frame() {
    let mux = Muxer::new(lc_stereo_48k()).unwrap();
    let mut bytes = Vec::new();
    mux.write_frame(&[9, 8, 7, 6], &mut bytes).unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let frame = demux.poll_frame().unwrap().expect("frame");
    assert_eq!(&frame[..], &[9, 8, 7, 6]);
    assert_eq!(demux.config(), Some(lc_stereo_48k()));
    assert!(demux.poll_frame().unwrap().is_none());
}

#[test]
fn roundtrips_multiple_back_to_back_frames() {
    let mux = Muxer::new(lc_stereo_48k()).unwrap();
    let mut bytes = Vec::new();
    mux.write_frame(&[1], &mut bytes).unwrap();
    mux.write_frame(&[2, 2], &mut bytes).unwrap();
    mux.write_frame(&[3, 3, 3], &mut bytes).unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.poll_frame().unwrap(), Some(Bytes::from_static(&[1])));
    assert_eq!(
        demux.poll_frame().unwrap(),
        Some(Bytes::from_static(&[2, 2]))
    );
    assert_eq!(
        demux.poll_frame().unwrap(),
        Some(Bytes::from_static(&[3, 3, 3]))
    );
    assert!(demux.poll_frame().unwrap().is_none());
}

#[test]
fn waits_for_more_bytes_on_partial_frame() {
    let mux = Muxer::new(lc_stereo_48k()).unwrap();
    let mut bytes = Vec::new();
    mux.write_frame(&[1, 2, 3, 4, 5], &mut bytes).unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes[..5]); // header not fully delivered yet
    assert!(demux.poll_frame().unwrap().is_none());

    demux.push_bytes(&bytes[5..]);
    let frame = demux.poll_frame().unwrap().expect("frame");
    assert_eq!(&frame[..], &[1, 2, 3, 4, 5]);
}

#[test]
fn rejects_bad_sync_word() {
    let mut demux = Demuxer::new();
    demux.push_bytes(&[0x00; 7]);
    assert!(demux.poll_frame().is_err());
}
