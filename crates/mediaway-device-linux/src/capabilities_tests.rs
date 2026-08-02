//! Unit tests for Linux capability/permission probing (sibling of `capabilities.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;

#[test]
fn loopback_and_process_loopback_are_not_implemented() {
    // Pure logic — no real probe for these kinds (no backend this session).
    for kind in [DeviceKind::Loopback, DeviceKind::ProcessLoopback] {
        assert_eq!(
            support(kind),
            Support::Unavailable(Unavailable::NotImplemented)
        );
        assert_eq!(request_permission(kind), Ok(PermissionState::NotSupported));
    }
}

#[test]
fn screen_support_or_skip() {
    // Real path: D-Bus round trip to `org.freedesktop.portal.Desktop`. Expected
    // to skip (report `NoDeviceFound`) in this WSL2/CI environment — no
    // portal-capable desktop session exists here. See crate ADR-0001 § Zero
    // runtime verification: this test has never observed a real `Supported`
    // result in this development session; it exists so the real path gets
    // exercised wherever a portal-capable Linux desktop later runs this suite.
    match support(DeviceKind::Screen) {
        Support::Supported => {}
        other => eprintln!("skip: screen portal unavailable here ({other:?})"),
    }
}

#[test]
fn screen_permission_probe_or_skip() {
    match request_permission(DeviceKind::Screen) {
        Ok(state) => eprintln!("screen permission probe: {state:?}"),
        Err(e) => eprintln!("skip: screen permission probe failed ({e:?})"),
    }
}

/// Same portal D-Bus round trip as `screen_support_or_skip` (see that test's
/// doc for why this is expected to skip in this environment).
#[test]
fn window_support_or_skip() {
    match support(DeviceKind::Window) {
        Support::Supported => {}
        other => eprintln!("skip: window portal unavailable here ({other:?})"),
    }
}

#[test]
fn window_permission_probe_or_skip() {
    match request_permission(DeviceKind::Window) {
        Ok(state) => eprintln!("window permission probe: {state:?}"),
        Err(e) => eprintln!("skip: window permission probe failed ({e:?})"),
    }
}

/// Real path: `V4L2` node enumeration. WSL2 has no `/dev/video*` nodes at all
/// (confirmed this session) — expected to report `NoDeviceFound` here. See
/// crate ADR-0002 § Zero runtime verification.
#[test]
fn camera_support_or_skip() {
    match support(DeviceKind::Camera) {
        Support::Supported => {}
        other => eprintln!("skip: no camera devices found ({other:?})"),
    }
}

#[test]
fn camera_permission_probe_or_skip() {
    match request_permission(DeviceKind::Camera) {
        Ok(state) => eprintln!("camera permission probe: {state:?}"),
        Err(e) => eprintln!("skip: camera permission probe failed ({e:?})"),
    }
}

/// Real path: connects to the local `PipeWire` daemon socket. WSL2 has no
/// running `PipeWire` daemon (confirmed this session) — expected to report
/// `NoDeviceFound` here. See crate ADR-0004 § Zero runtime verification.
#[test]
fn microphone_support_or_skip() {
    match support(DeviceKind::Microphone) {
        Support::Supported => {}
        other => eprintln!("skip: PipeWire daemon unavailable here ({other:?})"),
    }
}

#[test]
fn microphone_permission_probe_or_skip() {
    match request_permission(DeviceKind::Microphone) {
        Ok(state) => eprintln!("microphone permission probe: {state:?}"),
        Err(e) => eprintln!("skip: microphone permission probe failed ({e:?})"),
    }
}
