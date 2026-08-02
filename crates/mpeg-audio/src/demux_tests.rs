//! Unit tests for MPEG audio (Layer III) demux.

#![cfg(test)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::Demuxer;
use crate::mux::Muxer;
use crate::types::{ChannelMode, FrameHeader, MpegVersion};

fn mpeg1_128k_44k1_stereo() -> FrameHeader {
    FrameHeader {
        version: MpegVersion::Mpeg1,
        bitrate_kbps: 128,
        sample_rate: 44_100,
        channel_mode: ChannelMode::Stereo,
    }
}

#[test]
fn roundtrips_single_frame() {
    let header = mpeg1_128k_44k1_stereo();
    let mux = Muxer::new(header).unwrap();
    let body = vec![0x42; header.frame_len(false) - 4];
    let mut bytes = Vec::new();
    mux.write_frame(&body, false, &mut bytes).unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let frame = demux.poll_frame().unwrap().expect("frame");
    assert_eq!(&frame[..], &body[..]);
    assert_eq!(demux.header(), Some(header));
    assert!(demux.poll_frame().unwrap().is_none());
}

#[test]
fn roundtrips_multiple_back_to_back_frames_with_mixed_padding() {
    let header = mpeg1_128k_44k1_stereo();
    let mux = Muxer::new(header).unwrap();
    let body_no_pad = vec![1u8; header.frame_len(false) - 4];
    let body_pad = vec![2u8; header.frame_len(true) - 4];

    let mut bytes = Vec::new();
    mux.write_frame(&body_no_pad, false, &mut bytes).unwrap();
    mux.write_frame(&body_pad, true, &mut bytes).unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(
        demux.poll_frame().unwrap().as_deref(),
        Some(&body_no_pad[..])
    );
    assert_eq!(demux.poll_frame().unwrap().as_deref(), Some(&body_pad[..]));
    assert!(demux.poll_frame().unwrap().is_none());
}

#[test]
fn waits_for_more_bytes_on_partial_frame() {
    let header = mpeg1_128k_44k1_stereo();
    let mux = Muxer::new(header).unwrap();
    let body = vec![7u8; header.frame_len(false) - 4];
    let mut bytes = Vec::new();
    mux.write_frame(&body, false, &mut bytes).unwrap();

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes[..4]); // header only, no body yet
    assert!(demux.poll_frame().unwrap().is_none());

    demux.push_bytes(&bytes[4..]);
    assert!(demux.poll_frame().unwrap().is_some());
}

#[test]
fn rejects_bad_sync_word() {
    let mut demux = Demuxer::new();
    demux.push_bytes(&[0x00; 4]);
    assert!(demux.poll_frame().is_err());
}

#[test]
fn rejects_non_layer_iii() {
    let mut demux = Demuxer::new();
    // MPEG-1 (11), Layer I (11) instead of Layer III (01).
    demux.push_bytes(&[0xFF, 0xFE, 0x90, 0x00]);
    assert!(demux.poll_frame().is_err());
}
