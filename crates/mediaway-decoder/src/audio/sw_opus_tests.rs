#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    reason = "unit tests may unwrap"
)]

//! Tests for [`super::SwOpusAudioDecoder`] — real software decode (no OS/hardware
//! dependency), round-tripped against `mediaway-sw`'s own Opus encoder.

use mediaway_common::{AudioFrame, CodecKind, Packet, Rational, SampleFormat};
use mediaway_sw::opus::config::{OpusApplication, OpusDecoderConfig, OpusEncoderConfig};
use mediaway_sw::opus::encoder::OpusEncoder;

use super::sw_opus::SwOpusAudioDecoder;
use crate::AudioDecoder;

fn cfg() -> OpusDecoderConfig {
    OpusDecoderConfig::new(48_000, 2, Rational::new(1, 50))
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

/// Drives `packets` through `dec` purely via the [`AudioDecoder`] trait bound — proves
/// the concrete type satisfies the trait, not just its own inherent methods (used
/// identically against `WmfOpusDecoder` in `windows::wmf::opus_tests`).
fn decode_all_via_trait<D: AudioDecoder>(dec: &mut D, packets: &[Packet]) -> u64 {
    for pkt in packets {
        dec.push_packet(pkt).expect("push via trait");
    }
    dec.flush().expect("flush via trait");
    let mut decoded_samples = 0u64;
    while let Some(frame) = dec.poll_frame().expect("poll via trait") {
        decoded_samples += frame.data.len() as u64 / (4 * u64::from(frame.channels));
    }
    decoded_samples
}

#[test]
fn stream_info_reports_opus() {
    let dec = SwOpusAudioDecoder::open(&cfg()).expect("open sw opus decoder");
    let info = dec.stream_info();
    assert_eq!(info.codec(), CodecKind::Opus);
    assert_eq!(info.sample_rate(), Some(48_000));
    assert_eq!(info.channels(), Some(2));
}

#[test]
fn encodes_with_sw_encoder_and_decodes_via_audio_decoder_trait() {
    let sample_rate = 48_000;
    let channels = 2_u16;

    let mut enc = OpusEncoder::open(&OpusEncoderConfig {
        sample_rate,
        channels,
        application: OpusApplication::Audio,
        time_base: Rational::new(1, 50),
        bitrate_bps: Some(64_000),
        inband_fec: false,
        packet_loss_percent: 0,
    })
    .expect("open sw opus encoder");

    let frame_samples = (sample_rate / 50) as usize;
    enc.push_frame(&sine_frame(sample_rate, channels, frame_samples, 440.0))
        .expect("push frame");

    let mut packets = Vec::new();
    while let Some(pkt) = enc.poll_packet().expect("poll packet") {
        packets.push(pkt);
    }
    enc.flush().expect("flush encoder");
    while let Some(pkt) = enc.poll_packet().expect("poll packet") {
        packets.push(pkt);
    }
    assert!(!packets.is_empty(), "encoder produced no packets");

    let mut dec = SwOpusAudioDecoder::open(&cfg()).expect("open sw opus decoder");
    let decoded_samples = decode_all_via_trait(&mut dec, &packets);
    assert!(
        decoded_samples >= frame_samples as u64 / 2,
        "decoded only {decoded_samples} samples (expected >= {frame_samples})"
    );
}
