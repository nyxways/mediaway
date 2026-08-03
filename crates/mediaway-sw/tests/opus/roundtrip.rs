//! Integration round-trip: encode a synthetic sine-wave PCM buffer with
//! [`OpusEncoder`], decode it back with [`OpusDecoder`], and verify the
//! decoded output is a plausible reconstruction (Opus is lossy, so this
//! checks RMS energy similarity and sample count — not byte-exact equality).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    reason = "integration test: unwrap/expect allowed per testing.md; sample indices/counts \
              here are tiny (hundreds of samples) so f32/i64 casts never lose meaningful precision"
)]

use mediaway_common::{AudioFrame, Bytes, Packet, Rational, SampleFormat};
use mediaway_sw_opus::config::{OpusApplication, OpusDecoderConfig, OpusEncoderConfig};
use mediaway_sw_opus::decoder::OpusDecoder;
use mediaway_sw_opus::encoder::OpusEncoder;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
// 20ms frame at 48kHz.
const FRAME_SAMPLES: usize = 960;
const FRAME_COUNT: usize = 25; // 500ms of audio.

fn sine_pcm_frames() -> Vec<AudioFrame> {
    (0..FRAME_COUNT)
        .map(|frame_idx| {
            let mut data = Vec::with_capacity(FRAME_SAMPLES * usize::from(CHANNELS) * 4);
            for i in 0..FRAME_SAMPLES {
                let sample_idx = frame_idx * FRAME_SAMPLES + i;
                let t = sample_idx as f32 / SAMPLE_RATE as f32;
                let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
                for _ in 0..CHANNELS {
                    data.extend_from_slice(&v.to_le_bytes());
                }
            }
            AudioFrame {
                pts: frame_idx as i64,
                duration: FRAME_SAMPLES as u64,
                sample_rate: SAMPLE_RATE,
                channels: CHANNELS,
                format: SampleFormat::F32,
                data: Bytes::from(data),
            }
        })
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

fn pcm_f32(data: &Bytes) -> Vec<f32> {
    data.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

#[test]
fn encode_then_decode_reconstructs_similar_energy_sine_wave() {
    let enc_config = OpusEncoderConfig::new(
        SAMPLE_RATE,
        CHANNELS,
        OpusApplication::Audio,
        Rational::new(1, 50),
    );
    let dec_config = OpusDecoderConfig::new(SAMPLE_RATE, CHANNELS, Rational::new(1, 50));

    let mut encoder = OpusEncoder::open(&enc_config).expect("open encoder");
    let mut decoder = OpusDecoder::open(&dec_config).expect("open decoder");

    let mut packets: Vec<Packet> = Vec::new();
    for frame in sine_pcm_frames() {
        encoder.push_frame(&frame).expect("push_frame");
        while let Some(packet) = encoder.poll_packet().expect("poll_packet") {
            packets.push(packet);
        }
    }
    encoder.flush().expect("flush encoder");

    assert_eq!(
        packets.len(),
        FRAME_COUNT,
        "one packet per pushed frame in this synchronous design"
    );
    for packet in &packets {
        assert!(
            !packet.payload.is_empty(),
            "opus should never emit an empty packet for real audio"
        );
    }

    let mut decoded_samples: Vec<f32> = Vec::new();
    for packet in &packets {
        decoder.push_packet(packet).expect("push_packet");
        while let Some(frame) = decoder.poll_frame().expect("poll_frame") {
            assert_eq!(frame.format, SampleFormat::F32);
            assert_eq!(frame.data.len(), FRAME_SAMPLES * usize::from(CHANNELS) * 4);
            decoded_samples.extend(pcm_f32(&frame.data));
        }
    }
    decoder.flush().expect("flush decoder");

    let expected_sample_count = FRAME_COUNT * FRAME_SAMPLES * usize::from(CHANNELS);
    assert_eq!(decoded_samples.len(), expected_sample_count);

    let original_samples: Vec<f32> = sine_pcm_frames()
        .iter()
        .flat_map(|f| pcm_f32(&f.data))
        .collect();

    let original_rms = rms(&original_samples);
    let decoded_rms = rms(&decoded_samples);
    // Opus is lossy but this is a clean, well-within-band sine wave at a
    // reasonable implicit bitrate — RMS energy should survive within a
    // generous tolerance, not be near-silent or wildly amplified.
    let ratio = decoded_rms / original_rms;
    assert!(
        (0.5..1.5).contains(&ratio),
        "decoded RMS {decoded_rms} should be close to original RMS {original_rms} (ratio {ratio})"
    );
}

#[test]
fn empty_packet_round_trips_as_packet_loss_concealment() {
    let dec_config = OpusDecoderConfig::new(SAMPLE_RATE, CHANNELS, Rational::new(1, 50));
    let mut decoder = OpusDecoder::open(&dec_config).expect("open decoder");

    let plc_packet = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: FRAME_SAMPLES as u64,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::new(),
    };
    decoder.push_packet(&plc_packet).expect("push_packet (PLC)");
    let frame = decoder
        .poll_frame()
        .expect("poll_frame")
        .expect("one concealed frame");
    assert_eq!(frame.format, SampleFormat::F32);
    assert!(!frame.data.is_empty());
}
