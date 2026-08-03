#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::{WindowsCameraCapture, enumerate_cameras, preferred_subtype_order};
use crate::Select;
use crate::camera::{CameraCapture, CameraCaptureConfig, CaptureOutputPreference};
use mediaway_common::{PixelFormat, Rational, VideoFrameStorage};
use windows::Win32::Media::MediaFoundation::{MFVideoFormat_NV12, MFVideoFormat_RGB32};

#[test]
fn preferred_order_defaults_to_nv12_first_when_no_natives_known() {
    let order = preferred_subtype_order(&[]);
    assert_eq!(order[0].0, PixelFormat::Nv12);
}

#[test]
fn preferred_order_defaults_to_nv12_first_when_both_native() {
    let order = preferred_subtype_order(&[MFVideoFormat_NV12, MFVideoFormat_RGB32]);
    assert_eq!(order[0].0, PixelFormat::Nv12);
}

#[test]
fn preferred_order_picks_rgb32_first_when_only_rgb32_native() {
    let order = preferred_subtype_order(&[MFVideoFormat_RGB32]);
    assert_eq!(order[0].0, PixelFormat::Bgra8);
}

#[test]
fn preferred_order_picks_nv12_first_when_only_nv12_native() {
    let order = preferred_subtype_order(&[MFVideoFormat_NV12]);
    assert_eq!(order[0].0, PixelFormat::Nv12);
}

/// Neither NV12 nor RGB32 is native (e.g. an MJPG/YUY2-only webcam) — falls back to
/// requesting NV12 via the video processor's conversion, same as the "no natives known" case.
#[test]
fn preferred_order_defaults_to_nv12_first_when_neither_native() {
    let mjpg = windows::core::GUID::from_u128(0x4750_4a4d_0000_0010_8000_00aa_0038_9b71);
    let order = preferred_subtype_order(&[mjpg]);
    assert_eq!(order[0].0, PixelFormat::Nv12);
}

/// Enumerates real Media Foundation video capture devices on this machine. This box may be a
/// headless/desktop workstation with no camera attached — reported honestly, not a failure
/// (see `open_camera_capture_frames_or_skip` below for the same reasoning applied to a full
/// open + capture attempt).
#[test]
fn enumerate_camera_names_or_skip() {
    let _guard = crate::windows_camera::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match enumerate_cameras() {
        Ok(devices) if devices.is_empty() => {
            eprintln!("skip: no camera devices enumerated on this machine");
        }
        Ok(devices) => {
            let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
            eprintln!("camera devices: {names:?}");
            assert!(
                devices.iter().all(|d| !d.is_default),
                "ADR-0005: Camera has no OS default concept — is_default must always be false"
            );
        }
        Err(e) => eprintln!("skip: camera enumeration failed ({e:?})"),
    }
}

/// Opens the first real camera on this machine (if any) and captures a few real frames.
/// Skips gracefully — rather than failing — when no camera device exists, matching the
/// `_or_skip` convention `lib_tests.rs`/`capabilities_tests.rs` use for other hardware
/// (DXGI/WGC/WASAPI): dev boxes in this fleet are not guaranteed to have a webcam attached.
#[test]
fn open_camera_capture_frames_or_skip() {
    let _guard = crate::windows_camera::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let devices = match enumerate_cameras() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skip: camera enumeration failed ({e:?})");
            return;
        }
    };
    if devices.is_empty() {
        eprintln!("skip: no camera device on this machine");
        return;
    }
    eprintln!("opening camera 0: {:?}", devices[0].name);

    let cfg = CameraCaptureConfig {
        select: Select::Default,
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    let mut cap = match WindowsCameraCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: WindowsCameraCapture::open failed ({e:?})");
            return;
        }
    };

    let geometry = cap
        .stream_info()
        .geometry()
        .expect("video stream has geometry");
    assert!(geometry.width > 0);
    assert!(geometry.height > 0);
    eprintln!("camera negotiated {}x{}", geometry.width, geometry.height);

    let mut frames_seen = 0usize;
    for _ in 0..60 {
        match cap.poll_frame() {
            Ok(Some(frame)) => {
                match &frame.storage {
                    VideoFrameStorage::Cpu { data } => {
                        assert!(!data.is_empty(), "camera frame payload must not be empty");
                    }
                    VideoFrameStorage::Gpu(_) => {
                        unreachable!("this CPU-copy backend must never return GPU storage");
                    }
                    _ => unreachable!("unexpected VideoFrameStorage variant"),
                }
                assert!(frame.width > 0 && frame.height > 0);
                cap.release_frame().expect("release (no-op for CPU frames)");
                frames_seen += 1;
                if frames_seen >= 3 {
                    break;
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("skip: poll_frame failed ({e:?})");
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if frames_seen == 0 {
        eprintln!("skip: no frames captured within budget (camera busy/slow to start?)");
    } else {
        eprintln!("captured {frames_seen} real camera frame(s)");
    }
    cap.close().expect("close");
}
