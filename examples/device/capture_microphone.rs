//! Microphone capture in isolation — no encoding, no muxing.
//!
//! Opens the default microphone and polls a couple of seconds of PCM,
//! reporting sample rate/channels and total samples captured. For encoding
//! captured audio, see `pipeline/screen_record.rs` (mic → AAC → mp4 track).
//!
//! Run:
//! ```text
//! cargo run --example capture_microphone
//! ```

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "example demonstrates the happy path with console output"
)]

use mediaway::platform;
use mediaway_common::Rational;
use mediaway_device::audio::AudioCaptureConfig;
use std::time::{Duration, Instant};

const CAPTURE_SECS: u64 = 2;

fn main() {
    let cfg = AudioCaptureConfig::microphone(Rational::new(1, 48_000));
    let mut mic = match platform::Microphone::open(&cfg) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("capture_microphone: open failed ({e}) — platform not supported yet");
            return;
        }
    };

    let sample_rate = mic.stream_info().sample_rate();
    let channels = mic.stream_info().channels();
    println!("capture_microphone: {sample_rate:?} Hz, {channels:?} channel(s)");

    let deadline = Instant::now() + Duration::from_secs(CAPTURE_SECS);
    let mut frame_count = 0u32;
    let mut sample_count = 0usize;
    while Instant::now() < deadline {
        match mic.poll_frame() {
            Ok(Some(frame)) => {
                frame_count += 1;
                sample_count += frame.data.len();
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("capture_microphone: capture error ({e}), stopping");
                break;
            }
        }
    }

    mic.close().ok();
    println!(
        "capture_microphone: {frame_count} buffers, {sample_count} PCM bytes in {CAPTURE_SECS}s"
    );
}
