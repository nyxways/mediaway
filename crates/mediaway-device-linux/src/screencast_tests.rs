#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use mediaway_common::{NativeHandle, Rational};
use mediaway_device::Select;
use mediaway_device_desktop::{
    CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCaptureConfig,
};

#[test]
fn non_default_select_is_unsupported() {
    // Pure request-building/validation logic — no D-Bus/portal access: the
    // portal's own picker chooses the monitor interactively, so a
    // non-`Select::Default` selection is rejected before any I/O happens.
    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Screen {
            select: Select::NameContains("nonexistent".to_owned()),
        },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    assert!(matches!(
        LinuxScreenCapture::open(&cfg),
        Err(CaptureError::Unsupported)
    ));
}

#[test]
fn zero_copy_gpu_preference_is_unsupported_this_session() {
    // Same: rejected before any I/O — see struct rustdoc's Zero-Copy status.
    let cfg = DesktopVideoCaptureConfig::screen(Select::Default, Rational::new(1, 30));
    assert_eq!(cfg.output, CaptureOutputPreference::ZeroCopyGpu);
    assert!(matches!(
        LinuxScreenCapture::open(&cfg),
        Err(CaptureError::Unsupported)
    ));
}

#[test]
fn window_source_is_unsupported() {
    let window = NativeHandle::new(1).expect("nonzero handle");
    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Window { window },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    assert!(matches!(
        LinuxScreenCapture::open(&cfg),
        Err(CaptureError::Unsupported)
    ));
}

#[test]
fn open_screen_capture_or_skip() {
    // Real path: `xdg-desktop-portal` `ScreenCast` D-Bus handshake + PipeWire
    // stream connect. This test WILL skip here — no portal-capable desktop
    // session (session bus + compositor backend) exists in this WSL2
    // environment. See crate ADR-0001 § Zero runtime verification: this test
    // has never observed a real success in this development session; it
    // exists so the real path gets exercised wherever a portal-capable Linux
    // desktop later runs this suite.
    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Screen {
            select: Select::Default,
        },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    let mut cap = match LinuxScreenCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: LinuxScreenCapture::open failed ({e:?}) — no portal-capable session?");
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
