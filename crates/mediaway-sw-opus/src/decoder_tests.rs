#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    reason = "test module may unwrap; sample indices here are tiny (hundreds of samples) so \
              the f32 cast never loses meaningful precision"
)]

use super::*;
use crate::config::{OpusApplication, OpusEncoderConfig};
use crate::encoder::OpusEncoder;
use mediaway_common::Rational;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 1;
const FRAME_SAMPLES: usize = 960;

fn encoder_config() -> OpusEncoderConfig {
    OpusEncoderConfig::new(
        SAMPLE_RATE,
        CHANNELS,
        OpusApplication::Voip,
        Rational::new(1, 50),
    )
}

fn decoder_config() -> OpusDecoderConfig {
    OpusDecoderConfig::new(SAMPLE_RATE, CHANNELS, Rational::new(1, 50))
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
    OpusDecoder::open(&decoder_config()).expect("open");
}

#[test]
fn open_rejects_non_legal_frame_duration() {
    let mut cfg = decoder_config();
    cfg.time_base = Rational::new(1, 700);
    let err = OpusDecoder::open(&cfg).unwrap_err();
    assert!(matches!(err, OpusError::InvalidFrameDuration { .. }));
}

#[test]
fn push_packet_decodes_encoder_output_to_expected_sample_count() {
    let mut enc = OpusEncoder::open(&encoder_config()).expect("open encoder");
    let mut dec = OpusDecoder::open(&decoder_config()).expect("open decoder");

    let frame = sine_frame(FRAME_SAMPLES, CHANNELS);
    enc.push_frame(&frame).expect("push_frame");
    let packet = enc.poll_packet().expect("poll_packet").expect("one packet");

    dec.push_packet(&packet).expect("push_packet");
    let decoded = dec.poll_frame().expect("poll_frame").expect("one frame");
    assert_eq!(decoded.format, SampleFormat::F32);
    assert_eq!(
        decoded.data.len(),
        FRAME_SAMPLES * usize::from(CHANNELS) * 4
    );
    assert!(dec.poll_frame().expect("poll_frame").is_none());
}

#[test]
fn push_packet_treats_empty_payload_as_packet_loss_concealment() {
    let mut dec = OpusDecoder::open(&decoder_config()).expect("open");
    let packet = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: FRAME_SAMPLES as u64,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::new(),
    };
    dec.push_packet(&packet).expect("push_packet (PLC)");
    let decoded = dec.poll_frame().expect("poll_frame").expect("one frame");
    assert_eq!(decoded.data.len() % 4, 0);
}

#[test]
fn push_packet_rejects_after_flush() {
    let mut dec = OpusDecoder::open(&decoder_config()).expect("open");
    dec.flush().expect("flush");
    let packet = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::new(),
    };
    let err = dec.push_packet(&packet).unwrap_err();
    assert_eq!(err, OpusError::Closed);
}

#[test]
fn stream_info_reports_opus_codec() {
    let dec = OpusDecoder::open(&decoder_config()).expect("open");
    assert_eq!(dec.stream_info().codec(), CodecKind::Opus);
}
