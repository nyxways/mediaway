#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::cast_precision_loss,
    reason = "unit tests may unwrap"
)]

//! Tests for [`super::SwOpusAudioEncoder`] — encode→decode roundtrip via
//! `mediaway-sw`'s own Opus decoder (no OS/hardware involved).

use mediaway_common::{AudioFrame, CodecKind, Rational, SampleFormat};
use mediaway_sw::opus::config::OpusDecoderConfig as SwDecoderConfig;

use super::sw_opus::SwOpusAudioEncoder;
use crate::AudioEncoder;
use crate::audio::AudioEncoderConfig;

fn config() -> AudioEncoderConfig {
    AudioEncoderConfig {
        codec: CodecKind::Opus,
        sample_rate: 48_000,
        channels: 2,
        sample_format: SampleFormat::F32,
        time_base: Rational::new(1, 50),
        bitrate_bps: 64_000,
    }
}

fn sine_frame(sample_rate: u32, channels: u16, samples: usize, freq: f32) -> AudioFrame {
    let mut pcm: Vec<f32> = Vec::with_capacity(samples * usize::from(channels));
    for i in 0..samples {
        let t = i as f32 / sample_rate as f32;
        let s = (t * freq * std::f32::consts::TAU).sin();
        for _ in 0..channels {
            pcm.push(s);
        }
    }
    let data: Vec<u8> = pcm.iter().flat_map(|f| f.to_le_bytes()).collect();
    AudioFrame {
        pts: 0,
        duration: samples as u64,
        sample_rate,
        channels,
        format: SampleFormat::F32,
        data: data.into(),
    }
}

#[test]
fn encodes_sine_and_sw_decodes_roundtrip() {
    let cfg = config();
    let mut enc = SwOpusAudioEncoder::open(&cfg).expect("open sw opus encoder");

    let info = enc.stream_info();
    assert_eq!(info.codec(), CodecKind::Opus);

    let frame_samples = (cfg.sample_rate / 50) as usize;
    enc.push_frame(&sine_frame(
        cfg.sample_rate,
        cfg.channels,
        frame_samples,
        440.0,
    ))
    .expect("push frame");

    let mut packets = Vec::new();
    while let Some(pkt) = enc.poll_packet().expect("poll packet") {
        packets.push(pkt);
    }
    enc.flush().expect("flush");
    while let Some(pkt) = enc.poll_packet().expect("poll packet") {
        packets.push(pkt);
    }
    assert!(!packets.is_empty(), "encoder produced no packets");
    for p in &packets {
        assert_eq!(p.stream_id, 0);
        assert!(!p.payload.is_empty());
    }

    // SW decode the encoded packets back to PCM.
    let mut dec = mediaway_sw::opus::decoder::OpusDecoder::open(&SwDecoderConfig {
        sample_rate: cfg.sample_rate,
        channels: cfg.channels,
        time_base: cfg.time_base,
    })
    .expect("open sw opus decoder");
    let mut decoded = 0usize;
    for pkt in &packets {
        dec.push_packet(pkt).expect("sw decoder push");
        while let Some(frame) = dec.poll_frame().expect("sw decoder poll") {
            decoded += frame.data.len() / (4 * usize::from(cfg.channels));
        }
    }
    assert!(
        decoded >= frame_samples / 2,
        "decoded only {decoded} samples (expected >= {frame_samples})"
    );
}

#[test]
fn rejects_non_opus_and_non_f32_config() {
    let mut non_opus = config();
    non_opus.codec = CodecKind::Aac;
    assert!(SwOpusAudioEncoder::open(&non_opus).is_err());

    let mut non_f32 = config();
    non_f32.sample_format = SampleFormat::S16;
    assert!(SwOpusAudioEncoder::open(&non_f32).is_err());
}
