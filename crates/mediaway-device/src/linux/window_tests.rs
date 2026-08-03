#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use crate::Select;
use crate::desktop::{CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCaptureConfig};
use mediaway_common::{NativeHandle, Rational};

#[test]
fn non_window_source_is_unsupported() {
    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Screen {
            select: Select::Default,
        },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    assert!(matches!(
        LinuxWindowCapture::open(&cfg),
        Err(CaptureError::Unsupported)
    ));
}

#[test]
fn zero_copy_gpu_preference_is_unsupported_this_session() {
    // Same rejection point as `LinuxScreenCapture` — see `screencast::open_session`.
    let window = NativeHandle::new(1).expect("nonzero handle");
    let cfg = DesktopVideoCaptureConfig::window(window, Rational::new(1, 30));
    assert_eq!(cfg.output, CaptureOutputPreference::ZeroCopyGpu);
    assert!(matches!(
        LinuxWindowCapture::open(&cfg),
        Err(CaptureError::Unsupported)
    ));
}

/// Real path: `xdg-desktop-portal` `ScreenCast` handshake with
/// `SourceType::Window`. Expected to skip here — no portal-capable desktop
/// session exists in this WSL2 environment (same gap as
/// `screencast_tests::open_screen_capture_or_skip`). See crate
/// ADR-0003 § Zero runtime verification.
#[test]
fn open_window_capture_or_skip() {
    let window = NativeHandle::new(1).expect("nonzero handle");
    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Window { window },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    let mut cap = match LinuxWindowCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: LinuxWindowCapture::open failed ({e:?}) — no portal-capable session?");
            return;
        }
    };
    let geometry = cap
        .stream_info()
        .geometry()
        .expect("video stream has geometry");
    assert!(geometry.width > 0);
    assert!(geometry.height > 0);
    match cap.poll_frame() {
        Ok(Some(_frame)) => {
            cap.release_frame().expect("release");
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("skip: poll_frame failed ({e:?})");
            return;
        }
    }
    cap.close().expect("close");
}
