#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    reason = "test module may unwrap; sample indices here are tiny (hundreds of samples) so \
              the f32 cast never loses meaningful precision"
)]

use super::*;
use crate::opus::config::OpusApplication;
use mediaway_common::Rational;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 1;
// 20ms frame at 48kHz mono.
const FRAME_SAMPLES: usize = 960;

fn config() -> OpusEncoderConfig {
    OpusEncoderConfig::new(
        SAMPLE_RATE,
        CHANNELS,
        OpusApplication::Voip,
        Rational::new(1, 50),
    )
}

fn sine_frame(samples: usize, channels: u16) -> AudioFrame {
    let mut data = Vec::with_capacity(samples * usize::from(channels) * 4);
    for i in 0..samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        for _ in 0..channels {
            data.extend_from_slice(&v.to_le_bytes());
        }
    }
    AudioFrame {
        pts: 0,
        duration: samples as u64,
        sample_rate: SAMPLE_RATE,
        channels,
        format: SampleFormat::F32,
        data: Bytes::from(data),
    }
}

#[test]
fn open_succeeds_for_standard_20ms_config() {
    OpusEncoder::open(&config()).expect("open");
}

#[test]
fn open_rejects_non_legal_frame_duration() {
    let mut cfg = config();
    cfg.time_base = Rational::new(1, 700); // does not divide 48000 evenly
    let err = OpusEncoder::open(&cfg).unwrap_err();
    assert!(matches!(err, OpusError::InvalidFrameDuration { .. }));
}

#[test]
fn push_frame_encodes_and_poll_packet_drains_it() {
    let mut enc = OpusEncoder::open(&config()).expect("open");
    let frame = sine_frame(FRAME_SAMPLES, CHANNELS);
    enc.push_frame(&frame).expect("push_frame");
    let packet = enc.poll_packet().expect("poll_packet").expect("one packet");
    assert!(!packet.payload.is_empty());
    assert!(
        packet.payload.len() < frame.data.len(),
        "opus output should compress the input"
    );
    assert!(enc.poll_packet().expect("poll_packet").is_none());
}

#[test]
fn push_frame_rejects_wrong_sample_format() {
    let mut enc = OpusEncoder::open(&config()).expect("open");
    let mut frame = sine_frame(FRAME_SAMPLES, CHANNELS);
    frame.format = SampleFormat::S16;
    let err = enc.push_frame(&frame).unwrap_err();
    assert_eq!(err, OpusError::UnsupportedSampleFormat);
}

#[test]
fn push_frame_rejects_config_mismatch() {
    let mut enc = OpusEncoder::open(&config()).expect("open");
    let mut frame = sine_frame(FRAME_SAMPLES, CHANNELS);
    frame.sample_rate = 44_100;
    let err = enc.push_frame(&frame).unwrap_err();
    assert_eq!(err, OpusError::ConfigMismatch);
}

#[test]
fn push_frame_rejects_frame_size_mismatch() {
    let mut enc = OpusEncoder::open(&config()).expect("open");
    let frame = sine_frame(FRAME_SAMPLES / 2, CHANNELS);
    let err = enc.push_frame(&frame).unwrap_err();
    assert!(matches!(err, OpusError::FrameSizeMismatch { .. }));
}

#[test]
fn push_frame_rejects_after_flush() {
    let mut enc = OpusEncoder::open(&config()).expect("open");
    enc.flush().expect("flush");
    let frame = sine_frame(FRAME_SAMPLES, CHANNELS);
    let err = enc.push_frame(&frame).unwrap_err();
    assert_eq!(err, OpusError::Closed);
}

#[test]
fn stream_info_reports_opus_codec() {
    let enc = OpusEncoder::open(&config()).expect("open");
    assert_eq!(enc.stream_info().codec(), CodecKind::Opus);
}
