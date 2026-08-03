#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::suboptimal_flops,
    reason = "test modules may unwrap/panic-on-unexpected-variant; synthetic sample \
              indices/counts here are small enough that precision loss never actually \
              occurs, and the synthesized-speech formula below is written for readability \
              (fundamental + two harmonics), not for mul_add-optimized floating point"
)]

use super::*;
use mediaway_common::Bytes;

const SAMPLE_RATE: u32 = 16_000;
const BLOCK: usize = (SAMPLE_RATE / 100) as usize; // 160 samples per 10ms

fn f32_bytes(samples: &[f32]) -> Bytes {
    let mut buf = Vec::with_capacity(samples.len() * 4);
    for &sample in samples {
        buf.extend_from_slice(&sample.to_le_bytes());
    }
    Bytes::from(buf)
}

fn frame(samples: &[f32]) -> AudioFrame {
    AudioFrame {
        pts: 0,
        duration: samples.len() as u64,
        sample_rate: SAMPLE_RATE,
        channels: 1,
        format: SampleFormat::F32,
        data: f32_bytes(samples),
    }
}

#[test]
fn open_and_is_disabled_starts_enabled() {
    let vad = VoiceActivityDetector::open(SAMPLE_RATE).unwrap();
    assert!(!vad.is_disabled());
}

#[test]
fn analyze_rejects_non_f32_format() {
    let mut vad = VoiceActivityDetector::open(SAMPLE_RATE).unwrap();
    let mut bad = frame(&[0.0; BLOCK]);
    bad.format = SampleFormat::S16;
    let err = vad.analyze(&bad).unwrap_err();
    assert!(matches!(
        err,
        ApmError::UnsupportedSampleFormat(SampleFormat::S16)
    ));
}

#[test]
fn analyze_rejects_wrong_frame_length() {
    let mut vad = VoiceActivityDetector::open(SAMPLE_RATE).unwrap();
    let short = frame(&[0.0; BLOCK / 2]);
    let err = vad.analyze(&short).unwrap_err();
    match err {
        ApmError::FrameLengthMismatch { expected, actual } => {
            assert_eq!(expected, BLOCK);
            assert_eq!(actual, BLOCK / 2);
        }
        other => panic!("expected FrameLengthMismatch, got {other:?}"),
    }
}

#[test]
fn near_silent_frame_reports_low_probability() {
    let mut vad = VoiceActivityDetector::open(SAMPLE_RATE).unwrap();
    let silent = frame(&[0.0; BLOCK]);
    let mut last = 1.0_f32;
    for _ in 0..50 {
        last = vad.analyze(&silent).unwrap();
    }
    assert!(
        last < 0.3,
        "near-silent input should report low speech probability, got {last}"
    );
}

/// Regression guard for the sonora i16-scale gotcha (ADR § 5): `analyze`
/// must scale `[-1, 1]` samples by ×32768.0 before handing them to
/// `sonora`, or a realistic-amplitude "speech-like" signal reads as
/// permanent silence — indistinguishable from the near-silent case below.
/// If a future change drops the scale factor, this test's `speech_max`
/// collapses toward `0.0` and the final assertions fail.
#[test]
fn speech_like_frame_reports_higher_probability_than_near_silent() {
    let mut silent_vad = VoiceActivityDetector::open(SAMPLE_RATE).unwrap();
    let silent = frame(&[0.0; BLOCK]);
    let mut silent_max = 0.0_f32;
    for _ in 0..80 {
        silent_max = silent_max.max(silent_vad.analyze(&silent).unwrap());
    }

    let mut speech_vad = VoiceActivityDetector::open(SAMPLE_RATE).unwrap();
    let mut speech_max = 0.0_f32;
    for frame_i in 0..150_usize {
        let envelope = if frame_i < 5 {
            frame_i as f32 / 5.0
        } else if frame_i > 100 {
            ((150 - frame_i) as f32 / 50.0).max(0.0)
        } else {
            1.0
        };
        let samples: Vec<f32> = (0..BLOCK)
            .map(|n| {
                let t = (frame_i * BLOCK + n) as f32 / SAMPLE_RATE as f32;
                // Male-average-pitch fundamental + two harmonics — same
                // synthetic "speech-like" shape used to validate this
                // scaling boundary in the sibling project this ADR cites.
                let s = (t * 220.0 * std::f32::consts::TAU).sin() * 0.4
                    + (t * 440.0 * std::f32::consts::TAU).sin() * 0.25
                    + (t * 880.0 * std::f32::consts::TAU).sin() * 0.15;
                s * envelope
            })
            .collect();
        let p = speech_vad.analyze(&frame(&samples)).unwrap();
        speech_max = speech_max.max(p);
    }

    assert!(
        speech_max > 0.3,
        "speech-like signal should cross a real speech-probability threshold once \
         correctly i16-scaled internally, got max={speech_max} (silent baseline max={silent_max})"
    );
    assert!(
        speech_max > silent_max,
        "speech-like signal should score higher than near-silent input, got \
         speech={speech_max} silent={silent_max}"
    );
}
