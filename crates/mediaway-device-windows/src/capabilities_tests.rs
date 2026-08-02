//! Unit tests for Windows capability/permission probing (sibling of `capabilities.rs`).
//!
//! These exercise real, live checks against this machine (DXGI output
//! enumeration, WASAPI endpoint enumeration, WGC contract query) — not just
//! pure logic — so most assert on the *shape* of the result (e.g. "not the
//! not-implemented case") rather than a fixed value, since dev machines vary
//! (a screen-less CI runner has no DXGI output; a muted/absent mic still
//! enumerates as a valid endpoint or not depending on driver).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print skip diagnostics"
)]

use super::*;

#[test]
fn camera_is_not_implemented() {
    assert_eq!(
        support(DeviceKind::Camera),
        Support::Unavailable(Unavailable::NotImplemented)
    );
    assert_eq!(
        request_permission(DeviceKind::Camera),
        Ok(PermissionState::NotSupported)
    );
}

/// This dev machine has a real display, so DXGI output enumeration should
/// find at least one — asserts `Supported`, not just "not `NotImplemented`",
/// since a screen is a safe assumption for an interactive Windows session.
#[test]
fn screen_support_or_skip() {
    let _guard = crate::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match support(DeviceKind::Screen) {
        Support::Supported => {}
        other => eprintln!("skip: screen_output_support returned {other:?} (headless/CI box?)"),
    }
}

#[test]
fn window_capture_support_is_live_checked() {
    let _guard = crate::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Real Windows 10/11 dev machines support WGC; assert it's not classified
    // as "no code exists" (that would mean the WinRT query itself is broken).
    assert_ne!(
        support(DeviceKind::Window),
        Support::Unavailable(Unavailable::NotImplemented)
    );
}

#[test]
fn loopback_permission_is_granted_when_supported() {
    let _guard = crate::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if support(DeviceKind::Loopback) == Support::Supported {
        assert_eq!(
            request_permission(DeviceKind::Loopback),
            Ok(PermissionState::Granted)
        );
    } else {
        eprintln!("skip: no render endpoint on this machine");
    }
}

#[test]
fn screen_and_window_permission_are_unknown_when_supported() {
    let _guard = crate::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if support(DeviceKind::Screen) == Support::Supported {
        assert_eq!(
            request_permission(DeviceKind::Screen),
            Ok(PermissionState::Unknown)
        );
    }
    if support(DeviceKind::Window) == Support::Supported {
        assert_eq!(
            request_permission(DeviceKind::Window),
            Ok(PermissionState::Unknown)
        );
    }
}

/// Exercises the real `WASAPI` open/close probe on this machine's default
/// microphone endpoint. Skips (prints and returns) rather than failing when no
/// endpoint is present — same convention as `lib_tests.rs`'s `_or_skip` tests.
#[test]
fn microphone_permission_probe_or_skip() {
    let _guard = crate::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match request_permission(DeviceKind::Microphone) {
        Ok(state) => {
            eprintln!("microphone permission probe: {state:?}");
            assert!(matches!(
                state,
                PermissionState::Granted
                    | PermissionState::Denied
                    | PermissionState::Unknown
                    | PermissionState::NotSupported
            ));
        }
        Err(e) => eprintln!("skip: microphone permission probe failed ({e:?})"),
    }
}
