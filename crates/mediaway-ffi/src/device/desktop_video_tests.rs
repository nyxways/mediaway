//! Pure-logic unit tests for the GPU-handle enforcement rules added by
//! `adr/0003-gpu-handle-c-abi.md` §4/§5. None of these touch a real device or DXGI —
//! every case here is rejected before any backend/COM call, so no `HARDWARE_TEST_LOCK`
//! is needed (unlike this crate's Screen hardware round-trips, which live in
//! `mediaway-device-windows-desktop`'s own test suite).

#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::*;
use crate::device::status::MediawayDeviceStatus;
use crate::device::types::{MediawayGpuDeviceHandle, MediawayGpuDeviceKind, MediawayRational};

fn base_screen_config() -> MediawayDesktopCaptureConfig {
    mediaway_desktop_capture_config_screen(
        0,
        MediawayRational { num: 1, den: 30 },
        MediawayGpuDeviceHandle {
            kind: MediawayGpuDeviceKind::None,
            native: 0,
            webgpu_device_id: 0,
        },
    )
}

#[test]
fn open_screen_with_none_gpu_device_is_invalid_input_or_no_backend() {
    let config = base_screen_config();
    let mut out: *mut DesktopCaptureHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_desktop_capture_open(&raw const config, &raw mut out) };
    // Windows: `WindowsScreenCapture::open` rejects a `None` gpu_device before
    // touching any real device/COM object — deterministic, no hardware needed.
    // Non-Windows: this dispatch isn't wired at all (see `open_screen_capture`'s doc
    // comment) — `NoBackend`.
    assert!(matches!(
        status,
        MediawayDeviceStatus::InvalidInput | MediawayDeviceStatus::NoBackend
    ));
    assert!(out.is_null());
}

#[test]
fn open_rejects_null_config_as_invalid_argument() {
    let mut out: *mut DesktopCaptureHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_desktop_capture_open(std::ptr::null(), &raw mut out) };
    assert_eq!(status, MediawayDeviceStatus::InvalidArgument);
    assert!(out.is_null());
}

#[test]
fn close_on_null_is_a_no_op_ok() {
    let status = unsafe { mediaway_desktop_capture_close(std::ptr::null_mut()) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
}
