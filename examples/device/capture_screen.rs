//! Screen capture in isolation — no encoding, no muxing.
//!
//! Opens the primary display and polls a couple of seconds of frames,
//! reporting resolution and frame count. For turning captured frames into a
//! video file, see `pipeline/screen_record.rs`.
//!
//! Run:
//! ```text
//! cargo run --example capture_screen
//! ```

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "example demonstrates the happy path with console output"
)]

use mediaway::platform;
use mediaway_device::Select;
use mediaway_device::desktop::DesktopVideoCaptureConfig;
use std::time::{Duration, Instant};

const CAPTURE_SECS: u64 = 2;

fn main() {
    let cfg =
        DesktopVideoCaptureConfig::screen(Select::Default, mediaway_common::Rational::new(1, 30));
    let mut screen = match platform::ScreenCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("capture_screen: open failed ({e}) — platform not supported yet");
            return;
        }
    };

    let geometry = screen.stream_info().geometry();
    match geometry {
        Some(g) => println!("capture_screen: {}×{} display", g.width, g.height),
        None => println!("capture_screen: display opened (geometry not yet known)"),
    }

    let deadline = Instant::now() + Duration::from_secs(CAPTURE_SECS);
    let mut frame_count = 0u32;
    while Instant::now() < deadline {
        match screen.poll_frame() {
            Ok(Some(_frame)) => {
                frame_count += 1;
                let _ = screen.release_frame();
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("capture_screen: capture error ({e}), stopping");
                break;
            }
        }
    }

    screen.close().ok();
    println!("capture_screen: {frame_count} frames captured in {CAPTURE_SECS}s");
}
