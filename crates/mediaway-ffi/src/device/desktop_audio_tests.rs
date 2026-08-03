//! Pure-logic unit tests for `desktop_audio.rs`. No real device is touched here — every
//! case is rejected before any backend call, so no `HARDWARE_TEST_LOCK` is needed
//! (unlike this crate's real Loopback/`ProcessLoopback` round-trips, which live in
//! `mediaway-device-windows-desktop`'s own test suite).

#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::*;
use crate::device::status::MediawayDeviceStatus;
use crate::device::types::MediawayRational;

#[test]
fn open_rejects_null_config_as_invalid_argument() {
    let mut out: *mut DesktopAudioCaptureHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_desktop_audio_capture_open(std::ptr::null(), &raw mut out) };
    assert_eq!(status, MediawayDeviceStatus::InvalidArgument);
    assert!(out.is_null());
}

#[test]
fn open_rejects_nonzero_loopback_device_index_as_unsupported() {
    let mut config = mediaway_desktop_audio_capture_config_loopback(MediawayRational {
        num: 1,
        den: 48_000,
    });
    config.device_index = 1;
    let mut out: *mut DesktopAudioCaptureHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_desktop_audio_capture_open(&raw const config, &raw mut out) };
    assert_eq!(status, MediawayDeviceStatus::Unsupported);
    assert!(out.is_null());
}

#[test]
fn close_on_null_is_a_no_op_ok() {
    let status = unsafe { mediaway_desktop_audio_capture_close(std::ptr::null_mut()) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
}
