#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_possible_wrap,
    clippy::significant_drop_tightening,
    reason = "test modules may unwrap / print"
)]

use super::*;

// Only the pure dispatch branches (no real Camera2/AAudio FFI call) are exercised here — a real
// device/emulator is needed to verify `camera_support`'s live `ACameraManager` probe, and this
// crate's `android` CI job is compile+clippy only (see crate ADR-0001/0002/0003), so there is no
// environment in this workspace that would actually run a real-FFI test either way.

#[test]
fn screen_has_no_generic_support_probe() {
    assert_eq!(
        support(DeviceKind::Screen),
        Support::Unavailable(Unavailable::NotImplemented)
    );
}

#[test]
fn window_is_not_implemented() {
    assert_eq!(
        support(DeviceKind::Window),
        Support::Unavailable(Unavailable::NotImplemented)
    );
}

#[test]
fn microphone_reports_supported_unconditionally() {
    assert_eq!(support(DeviceKind::Microphone), Support::Supported);
}

#[test]
fn request_permission_screen_is_not_supported() {
    assert_eq!(
        request_permission(DeviceKind::Screen).unwrap(),
        PermissionState::NotSupported
    );
}

#[test]
fn request_permission_microphone_is_unknown() {
    assert_eq!(
        request_permission(DeviceKind::Microphone).unwrap(),
        PermissionState::Unknown
    );
}
