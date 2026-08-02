//! Unit tests for the MP3 facade adapter (sibling of `mp3.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use mpeg_audio::ChannelMode;

fn header() -> FrameHeader {
    FrameHeader {
        version: MpegVersion::Mpeg1,
        bitrate_kbps: 128,
        sample_rate: 44_100,
        channel_mode: ChannelMode::Stereo,
    }
}

#[test]
fn samples_per_frame_matches_layer3_standard() {
    assert_eq!(samples_per_frame(MpegVersion::Mpeg1), 1152);
    assert_eq!(samples_per_frame(MpegVersion::Mpeg2), 576);
    assert_eq!(samples_per_frame(MpegVersion::Mpeg25), 576);
}

#[test]
fn mux_then_demux_roundtrips_frame_and_synthesizes_timing() {
    let mux = Muxer::new(header()).expect("valid header");
    let frame_len = header().frame_len(false);
    let body = vec![0xAB; frame_len - 4];
    let mut bytes = Vec::new();
    mux.write_frame(&body, false, &mut bytes).expect("write");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let p1 = demux.poll_packet().expect("frame 1");
    assert_eq!(&p1.payload[..], &body[..]);
    assert_eq!(p1.pts, 0);
    assert_eq!(p1.duration, 1152);

    assert_eq!(demux.streams().len(), 1);
    assert_eq!(demux.streams()[0].codec(), CodecKind::Mp3);
    assert_eq!(demux.streams()[0].sample_rate(), Some(44_100));
    assert_eq!(demux.streams()[0].channels(), Some(2));
}

#[test]
fn mono_stream_reports_one_channel() {
    let mono_header = FrameHeader {
        channel_mode: ChannelMode::Mono,
        ..header()
    };
    let mux = Muxer::new(mono_header).expect("valid header");
    let frame_len = mono_header.frame_len(false);
    let body = vec![0u8; frame_len - 4];
    let mut bytes = Vec::new();
    mux.write_frame(&body, false, &mut bytes).expect("write");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    demux.poll_packet().expect("frame");
    assert_eq!(demux.streams()[0].channels(), Some(1));
}
