//! Camera capture in isolation — no encoding, no muxing.
//!
//! Opens the default camera and polls a couple of seconds of frames. Not yet
//! wired into `mediaway_pipeline::platform` (see the crate's roadmap), so
//! this reaches for the Windows backend directly — on every other platform
//! `mediaway-device-windows-camera` ships a stub that returns
//! `CaptureError::Unsupported`, which this example reports the same way it
//! reports "no camera attached" on Windows.
//!
//! Run:
//! ```text
//! cargo run --example capture_camera
//! ```

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "example demonstrates the happy path with console output"
)]

use mediaway_common::Rational;
use mediaway_device::Select;
use mediaway_device::camera::{CameraCapture, CameraCaptureConfig, CaptureOutputPreference};
use mediaway_device::windows_camera::WindowsCameraCapture;
use std::time::{Duration, Instant};

const CAPTURE_SECS: u64 = 2;

fn main() {
    let cfg = CameraCaptureConfig {
        select: Select::Default,
        time_base: Rational::new(1, 30),
        // The Media Foundation camera backend only implements CPU frame
        // delivery today — ZeroCopyGpu (the type's own default) is rejected.
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };

    let mut camera = match WindowsCameraCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("capture_camera: open failed ({e}) — no camera on this machine/platform?");
            return;
        }
    };

    if let Some(g) = camera.stream_info().geometry() {
        println!("capture_camera: {}×{}", g.width, g.height);
    }

    let deadline = Instant::now() + Duration::from_secs(CAPTURE_SECS);
    let mut frame_count = 0u32;
    while Instant::now() < deadline {
        match camera.poll_frame() {
            Ok(Some(_frame)) => frame_count += 1,
            Ok(None) => {}
            Err(e) => {
                eprintln!("capture_camera: capture error ({e}), stopping");
                break;
            }
        }
    }

    camera.close().ok();
    println!("capture_camera: {frame_count} frames captured in {CAPTURE_SECS}s");
}
