#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::suboptimal_flops,
    reason = "test module may unwrap; sample indices here are tiny (hundreds of samples) so \
              the f32 cast never loses meaningful precision; the synthetic-voice formulas \
              favor readability over mul_add"
)]

use super::*;
use crate::opus::config::{OpusApplication, OpusBandwidth, OpusSignal};
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

fn voiced(i: usize) -> f32 {
    // Speech-like: 140 Hz fundamental + two harmonics (enough for SILK to code as voiced).
    let t = i as f32 / SAMPLE_RATE as f32;
    let tau = std::f32::consts::TAU;
    0.3 * (140.0 * tau * t).sin() + 0.15 * (280.0 * tau * t).sin() + 0.08 * (420.0 * tau * t).sin()
}

#[test]
fn encode_float_into_writes_a_packet_without_a_session_frame() {
    let mut enc = OpusEncoder::open(&config()).expect("open");
    let pcm: Vec<f32> = (0..FRAME_SAMPLES).map(voiced).collect();
    let mut out = [0u8; 1275];
    let len = enc.encode_float_into(&pcm, &mut out).expect("encode");
    assert!(len > 2 && len < 400, "unexpected packet length {len}");
}

#[test]
fn encode_float_into_rejects_bad_lengths() {
    let mut enc = OpusEncoder::open(&config()).expect("open");
    let mut out = [0u8; 1275];
    // 7 samples is not a legal Opus frame duration.
    let err = enc.encode_float_into(&[0.0; 7], &mut out).unwrap_err();
    assert!(matches!(err, OpusError::Backend { .. }));
    assert!(enc.encode_float_into(&[], &mut out).is_err());
}

#[test]
fn setters_accept_the_voice_profile_and_report_lookahead() {
    let mut enc = OpusEncoder::open(&config()).expect("open");
    enc.set_bitrate(24_000).expect("bitrate");
    enc.set_complexity(10).expect("complexity");
    enc.set_signal(OpusSignal::Voice).expect("signal");
    enc.set_max_bandwidth(OpusBandwidth::Wide)
        .expect("bandwidth");
    enc.set_vbr(true).expect("vbr");
    enc.set_inband_fec(true).expect("fec");
    enc.set_packet_loss_percent(10).expect("loss");
    enc.set_dtx(true).expect("dtx");
    assert!(enc.set_complexity(11).is_err());
    let lookahead = enc.lookahead().expect("lookahead");
    assert!(
        (120..=480).contains(&lookahead),
        "lookahead {lookahead} samples"
    );
}

#[test]
fn dtx_turns_silence_into_tiny_packets() {
    let mut enc = OpusEncoder::open(&config()).expect("open");
    enc.set_dtx(true).expect("dtx");
    let silence = vec![0.0f32; FRAME_SAMPLES];
    let mut out = [0u8; 1275];
    let mut last = usize::MAX;
    for _ in 0..40 {
        last = enc.encode_float_into(&silence, &mut out).expect("encode");
    }
    assert!(last <= 2, "silent frame still {last} bytes with DTX on");
    assert!(enc.in_dtx().expect("in_dtx"));
}
