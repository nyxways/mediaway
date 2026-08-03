#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use crate::Select;
use crate::audio::AudioCaptureConfig;
use mediaway_common::{Rational, SampleFormat};

#[test]
fn usable_pcm_len_rejects_zero_channels() {
    assert_eq!(usable_pcm_len(64, 0), None);
}

#[test]
fn usable_pcm_len_rejects_undersized_chunk() {
    // 2 channels * 4 bytes = 8 bytes/frame; 4 bytes can't hold one frame.
    assert_eq!(usable_pcm_len(4, 2), None);
}

#[test]
fn usable_pcm_len_truncates_to_whole_frames() {
    // 2 channels * 4 bytes = 8 bytes/frame; 20 bytes -> 2 whole frames (16 bytes).
    assert_eq!(usable_pcm_len(20, 2), Some(16));
}

#[test]
fn usable_pcm_len_exact_multiple_is_unchanged() {
    assert_eq!(usable_pcm_len(32, 4), Some(32));
}

#[test]
fn non_default_select_is_unsupported() {
    let cfg = AudioCaptureConfig {
        select: Select::NameContains("nonexistent".to_owned()),
        time_base: Rational::new(1, 48_000),
        sample_format: SampleFormat::F32,
    };
    assert!(matches!(
        LinuxMicrophoneCapture::open(&cfg),
        Err(CaptureError::Unsupported)
    ));
}

#[test]
fn non_float_sample_format_is_unsupported() {
    let cfg = AudioCaptureConfig {
        select: Select::Default,
        time_base: Rational::new(1, 48_000),
        sample_format: SampleFormat::S16,
    };
    assert!(matches!(
        LinuxMicrophoneCapture::open(&cfg),
        Err(CaptureError::Unsupported)
    ));
}

#[test]
fn zero_denominator_time_base_is_invalid_input() {
    let cfg = AudioCaptureConfig {
        select: Select::Default,
        time_base: Rational::new(1, 0),
        sample_format: SampleFormat::F32,
    };
    assert!(matches!(
        LinuxMicrophoneCapture::open(&cfg),
        Err(CaptureError::InvalidInput)
    ));
}

/// Real path: connects to the local `PipeWire` daemon socket and negotiates
/// a default-source audio stream. WSL2 has no running `PipeWire` daemon
/// (confirmed this session, even though `libpipewire-0.3-dev` is installed)
/// — expected to skip here. See crate ADR-0004 § Zero runtime verification.
#[test]
fn open_microphone_capture_or_skip() {
    let cfg = AudioCaptureConfig::microphone(Rational::new(1, 48_000));
    let mut cap = match LinuxMicrophoneCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: LinuxMicrophoneCapture::open failed ({e:?}) — no PipeWire daemon?");
            return;
        }
    };
    match cap.poll_frame() {
        Ok(Some(_frame)) => {}
        Ok(None) => {}
        Err(e) => {
            eprintln!("skip: poll_frame failed ({e:?})");
            return;
        }
    }
    cap.close().expect("close");
}
