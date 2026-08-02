//! Unit tests for the WAV facade adapter (sibling of `wav.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn mux_then_parse_roundtrips_pcm() {
    let mut mux = Muxer::new(44_100, 2, 16);
    let pcm = [1u8, 2, 3, 4, 5, 6, 7, 8]; // 2 frames of 4-byte (2ch x 16-bit) PCM
    mux.push_packet(&Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::copy_from_slice(&pcm),
    });
    let bytes = mux.finish();
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");

    let (stream, packet) = parse(&bytes).expect("parse");
    assert_eq!(stream.sample_rate(), Some(44_100));
    assert_eq!(stream.channels(), Some(2));
    assert_eq!(stream.codec(), CodecKind::RawAudio);
    assert_eq!(&packet.payload[..], &pcm);
    assert_eq!(packet.duration, 2);
}

#[test]
fn parse_propagates_not_riff_wave_error() {
    assert!(matches!(parse(b"not a wav file"), Err(Error::NotRiffWave)));
}
