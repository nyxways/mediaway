//! Pure-logic unit tests for `camera.rs`. No real device is touched here — every case
//! is rejected before any backend call, so no `HARDWARE_TEST_LOCK` is needed (unlike
//! this crate's real Camera round-trips, which live in `mediaway-device-windows-camera`'s
//! own test suite).

#![allow(clippy::unwrap_used, reason = "unit tests")]

use std::mem::MaybeUninit;

use super::*;
use crate::device::status::MediawayDeviceStatus;
use crate::device::types::MediawayRational;

fn base_camera_config() -> MediawayCameraCaptureConfig {
    mediaway_camera_capture_config_default(0, MediawayRational { num: 1, den: 30 })
}

#[test]
fn capture_once_rejects_null_config_as_invalid_argument() {
    let mut frame = MaybeUninit::<MediawayCameraFrame>::uninit();
    let status =
        unsafe { mediaway_camera_capture_capture_once(std::ptr::null(), 10, frame.as_mut_ptr()) };
    assert_eq!(status, MediawayDeviceStatus::InvalidArgument);
}

#[test]
fn open_rejects_null_config_as_invalid_argument() {
    let mut out: *mut CameraCaptureHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_camera_capture_open(std::ptr::null(), &raw mut out) };
    assert_eq!(status, MediawayDeviceStatus::InvalidArgument);
    assert!(out.is_null());
}

#[test]
fn close_on_null_is_a_no_op_ok() {
    let status = unsafe { mediaway_camera_capture_close(std::ptr::null_mut()) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
}

#[test]
fn base_camera_config_has_no_gpu_field() {
    // `MediawayCameraCaptureConfig` intentionally has no `gpu_device` field — every
    // shipped Camera backend rejects Zero-Copy today, so the pre-split "reject a
    // non-NONE gpu_device on Camera" check is now a compile-time impossibility
    // instead of a runtime error path (same idiom as
    // `mediaway-device-windows-desktop`'s `null_window_handle_is_unrepresentable`).
    let config = base_camera_config();
    assert_eq!(config.device_index, 0);
}
