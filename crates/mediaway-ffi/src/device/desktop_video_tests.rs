//! Pure-logic unit tests for the GPU-handle enforcement rules added by
//! `adr/0003-gpu-handle-c-abi.md` §4/§5. None of these touch a real device or DXGI —
//! every case here is rejected before any backend/COM call, so no `HARDWARE_TEST_LOCK`
//! is needed (unlike this crate's Screen hardware round-trips, which live in
//! `mediaway-device-windows-desktop`'s own test suite).

#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::*;
use crate::device::status::MediawayDeviceStatus;
use crate::device::types::{
    MediawayCaptureBorder, MediawayCaptureCursor, MediawayFrameDimensions, MediawayGpuDeviceHandle,
    MediawayGpuDeviceKind, MediawayRational,
};

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

fn none_gpu() -> MediawayGpuDeviceHandle {
    MediawayGpuDeviceHandle {
        kind: MediawayGpuDeviceKind::None,
        native: 0,
        webgpu_device_id: 0,
    }
}

fn open(
    config: &MediawayDesktopCaptureConfig,
) -> (MediawayDeviceStatus, *mut DesktopCaptureHandle) {
    let mut out: *mut DesktopCaptureHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_desktop_capture_open(config, &raw mut out) };
    (status, out)
}

/// A zero-initialised tail must mean "as before ADR-0005": whole surface, no pointer, the OS
/// default border, native dimensions.
#[test]
fn new_fields_default_to_previous_behaviour() {
    for config in [
        base_screen_config(),
        mediaway_desktop_capture_config_window(
            0xABCD,
            MediawayRational { num: 1, den: 30 },
            none_gpu(),
        ),
    ] {
        assert_eq!(config.cursor, MediawayCaptureCursor::Excluded);
        assert_eq!(config.border, MediawayCaptureBorder::Shown);
        assert_eq!(config.dimensions, MediawayFrameDimensions::Native);
        assert!(!config.region_enabled);
        assert_eq!(
            (
                config.region_x,
                config.region_y,
                config.region_width,
                config.region_height
            ),
            (0, 0, 0, 0)
        );
    }
}

#[test]
fn window_config_carries_the_hwnd_and_no_screen_ordinal() {
    let config = mediaway_desktop_capture_config_window(
        0x1_0000_0001,
        MediawayRational { num: 1, den: 60 },
        none_gpu(),
    );
    assert_eq!(config.source_kind, MediawayDesktopCaptureSourceKind::Window);
    assert_eq!(config.window_handle, 0x1_0000_0001);
    assert_eq!(config.source_index, 0);
}

/// The C header, C#, Python, Node and C++ each hand-mirror this struct. Pin the layout so
/// a reordered or resized field fails here instead of corrupting a binding's config.
#[test]
fn config_layout_is_pinned() {
    use std::mem::{offset_of, size_of};
    type C = MediawayDesktopCaptureConfig;
    assert_eq!(offset_of!(C, source_kind), 0);
    assert_eq!(offset_of!(C, source_index), 4);
    let after_gpu = offset_of!(C, gpu_device) + size_of::<MediawayGpuDeviceHandle>();
    // Everything appended by ADR-0005 comes after `gpu_device`, in declaration order.
    assert!(offset_of!(C, window_handle) >= after_gpu);
    assert_eq!(offset_of!(C, window_handle) % 8, 0);
    assert!(offset_of!(C, cursor) > offset_of!(C, window_handle));
    assert!(offset_of!(C, border) > offset_of!(C, cursor));
    assert!(offset_of!(C, dimensions) > offset_of!(C, border));
    assert!(offset_of!(C, region_x) > offset_of!(C, dimensions));
    assert!(offset_of!(C, region_enabled) > offset_of!(C, region_height));
    assert_eq!(size_of::<MediawayCaptureCursor>(), 4);
    assert_eq!(size_of::<MediawayCaptureBorder>(), 4);
    assert_eq!(size_of::<MediawayFrameDimensions>(), 4);
}

#[test]
fn window_with_zero_hwnd_is_invalid_input_or_unsupported() {
    let config =
        mediaway_desktop_capture_config_window(0, MediawayRational { num: 1, den: 30 }, none_gpu());
    let (status, out) = open(&config);
    // Windows: an `HWND` of 0 names no window. Elsewhere: no `HWND` source exists at all.
    assert!(matches!(
        status,
        MediawayDeviceStatus::InvalidInput | MediawayDeviceStatus::Unsupported
    ));
    assert!(out.is_null());
}

#[test]
fn enabled_region_with_zero_size_is_invalid_input() {
    let mut config = base_screen_config();
    config.region_enabled = true;
    config.region_width = 0;
    config.region_height = 64;
    let (status, out) = open(&config);
    assert_eq!(status, MediawayDeviceStatus::InvalidInput);
    assert!(out.is_null());
}

/// `border` / `dimensions` are Window-only. Dropping them silently on a Screen config would
/// hide a caller mistake, so they are refused (ADR-0005 §6).
#[test]
fn window_only_options_on_a_screen_config_are_invalid_input() {
    let mut hidden = base_screen_config();
    hidden.border = MediawayCaptureBorder::Hidden;
    let mut cropped = base_screen_config();
    cropped.dimensions = MediawayFrameDimensions::EvenCropped;
    for config in [hidden, cropped] {
        let (status, out) = open(&config);
        assert_eq!(status, MediawayDeviceStatus::InvalidInput);
        assert!(out.is_null());
    }
}

/// DXGI cannot draw the pointer or crop: the request must come back `UNSUPPORTED`, not be
/// ignored. Both are refused before any device or COM call, so no hardware lock is needed.
#[cfg(windows)]
#[test]
fn screen_cursor_included_and_region_are_unsupported_on_dxgi() {
    let mut cursor = base_screen_config();
    cursor.cursor = MediawayCaptureCursor::Included;
    let mut region = base_screen_config();
    region.region_enabled = true;
    region.region_width = 64;
    region.region_height = 64;
    for config in [cursor, region] {
        let (status, out) = open(&config);
        assert_eq!(status, MediawayDeviceStatus::Unsupported);
        assert!(out.is_null());
    }
}

#[test]
fn border_hidden_rejects_null_arguments() {
    let mut hidden = false;
    let status =
        unsafe { mediaway_desktop_capture_border_hidden(std::ptr::null(), &raw mut hidden) };
    assert_eq!(status, MediawayDeviceStatus::InvalidArgument);
}

#[test]
fn region_out_of_bounds_maps_to_its_own_status() {
    use mediaway_device::CaptureError;
    let status = MediawayDeviceStatus::from(CaptureError::RegionOutOfBounds {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
        surface_width: 5,
        surface_height: 5,
    });
    assert_eq!(status, MediawayDeviceStatus::RegionOutOfBounds);
    assert_eq!(status as i32, 14);
}
