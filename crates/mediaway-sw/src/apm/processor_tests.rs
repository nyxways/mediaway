#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    reason = "test modules may unwrap; synthetic sample indices/counts here are small \
              enough that precision loss / wraparound never actually occurs"
)]

use super::*;
use mediaway_common::Bytes;
use sonora::config::{EchoCanceller, GainController2, NoiseSuppression};

const SAMPLE_RATE: u32 = 8_000;
const CHANNELS: u16 = 1;
const BLOCK: usize = (SAMPLE_RATE / 100) as usize; // 80 samples per 10ms at 8kHz

fn stream_format() -> AudioStreamFormat {
    AudioStreamFormat {
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        sample_format: SampleFormat::F32,
    }
}

fn f32_bytes(samples: &[f32]) -> Bytes {
    let mut buf = Vec::with_capacity(samples.len() * 4);
    for &sample in samples {
        buf.extend_from_slice(&sample.to_le_bytes());
    }
    Bytes::from(buf)
}

fn bytes_to_f32(data: &Bytes) -> Vec<f32> {
    data.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn frame(pts: i64, samples: &[f32]) -> AudioFrame {
    AudioFrame {
        pts,
        duration: samples.len() as u64,
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        format: SampleFormat::F32,
        data: f32_bytes(samples),
    }
}

fn open_default() -> AudioProcessor {
    AudioProcessor::open(ApmConfig::default(), stream_format(), stream_format())
        .expect("open with default (all-components-disabled) config")
}

#[test]
fn open_rejects_non_f32_capture_format() {
    let bad = AudioStreamFormat {
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        sample_format: SampleFormat::S16,
    };
    let err = AudioProcessor::open(ApmConfig::default(), bad, stream_format()).unwrap_err();
    assert!(matches!(
        err,
        ApmError::UnsupportedSampleFormat(SampleFormat::S16)
    ));
}

#[test]
fn open_rejects_non_f32_render_format() {
    let bad = AudioStreamFormat {
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        sample_format: SampleFormat::S32,
    };
    let err = AudioProcessor::open(ApmConfig::default(), stream_format(), bad).unwrap_err();
    assert!(matches!(
        err,
        ApmError::UnsupportedSampleFormat(SampleFormat::S32)
    ));
}

#[test]
fn poll_returns_none_until_a_full_10ms_block_accumulates() {
    let mut apm = open_default();

    // Half a block pushed — not enough for one 10ms block yet.
    apm.push_capture_frame(&frame(0, &[0.0; BLOCK / 2]))
        .unwrap();
    assert!(apm.poll_processed_frame().unwrap().is_none());

    // Remaining half completes the block.
    apm.push_capture_frame(&frame(0, &[0.0; BLOCK / 2]))
        .unwrap();
    let out = apm
        .poll_processed_frame()
        .unwrap()
        .expect("full block should be ready");
    assert_eq!(out.duration, BLOCK as u64);
    assert_eq!(out.data.len(), BLOCK * 4);
    assert_eq!(out.sample_rate, SAMPLE_RATE);
    assert_eq!(out.channels, CHANNELS);
    assert_eq!(out.format, SampleFormat::F32);

    // Block was consumed — nothing left to poll.
    assert!(apm.poll_processed_frame().unwrap().is_none());
}

#[test]
fn silence_in_produces_silence_out_with_all_components_disabled() {
    let mut apm = open_default();
    apm.push_capture_frame(&frame(0, &vec![0.0; BLOCK]))
        .unwrap();
    let out = apm
        .poll_processed_frame()
        .unwrap()
        .expect("block should be ready");
    let samples = bytes_to_f32(&out.data);
    assert!(samples.iter().all(|&s| s.abs() < 1e-6));
}

#[test]
fn full_processing_pipeline_handles_multiple_blocks_without_error() {
    let config = ApmConfig {
        echo_canceller: Some(EchoCanceller::default()),
        noise_suppression: Some(NoiseSuppression::default()),
        gain_controller2: Some(GainController2::default()),
        ..Default::default()
    };
    let mut apm = AudioProcessor::open(config, stream_format(), stream_format())
        .expect("open with AEC3+NS+AGC2 enabled");

    for i in 0..20_usize {
        let samples: Vec<f32> = (0..BLOCK)
            .map(|n| {
                let t = (i * BLOCK + n) as f32 / SAMPLE_RATE as f32;
                (t * 220.0 * std::f32::consts::TAU).sin() * 0.3
            })
            .collect();
        apm.push_render_frame(&frame(0, &samples)).unwrap();
        apm.push_capture_frame(&frame(0, &samples)).unwrap();
        let out = apm.poll_processed_frame().unwrap();
        assert!(out.is_some(), "block {i} should have produced output");
    }
    assert!(!apm.is_disabled());
}

#[test]
fn push_capture_frame_rejects_stream_format_mismatch() {
    let mut apm = open_default();
    let mismatched = AudioFrame {
        pts: 0,
        duration: 10,
        sample_rate: 16_000,
        channels: CHANNELS,
        format: SampleFormat::F32,
        data: f32_bytes(&[0.0; 10]),
    };
    let err = apm.push_capture_frame(&mismatched).unwrap_err();
    assert!(matches!(err, ApmError::StreamFormatMismatch { .. }));
}

#[test]
fn pts_advances_by_one_block_per_produced_frame() {
    let mut apm = open_default();
    apm.push_capture_frame(&frame(100, &vec![0.0; BLOCK]))
        .unwrap();
    apm.push_capture_frame(&frame(0, &vec![0.0; BLOCK]))
        .unwrap();

    let first = apm.poll_processed_frame().unwrap().expect("first block");
    assert_eq!(first.pts, 100);
    let second = apm.poll_processed_frame().unwrap().expect("second block");
    assert_eq!(second.pts, 100 + BLOCK as i64);
}
