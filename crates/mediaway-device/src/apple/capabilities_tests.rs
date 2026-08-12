#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_possible_wrap,
    clippy::significant_drop_tightening,
    reason = "test modules may unwrap / print"
)]

use super::*;

#[test]
fn map_authorization_authorized_is_granted() {
    assert_eq!(
        map_authorization(AVAuthorizationStatus::Authorized),
        PermissionState::Granted
    );
}

#[test]
fn map_authorization_denied_and_restricted_are_denied() {
    assert_eq!(
        map_authorization(AVAuthorizationStatus::Denied),
        PermissionState::Denied
    );
    assert_eq!(
        map_authorization(AVAuthorizationStatus::Restricted),
        PermissionState::Denied
    );
}

#[test]
fn map_authorization_not_determined_is_unknown() {
    assert_eq!(
        map_authorization(AVAuthorizationStatus::NotDetermined),
        PermissionState::Unknown
    );
}

#[test]
fn window_loopback_are_not_implemented() {
    assert_eq!(
        support(DeviceKind::Window),
        Support::Unavailable(Unavailable::NotImplemented)
    );
    assert_eq!(
        support(DeviceKind::Loopback),
        Support::Unavailable(Unavailable::NotImplemented)
    );
}

#[test]
fn camera_and_microphone_report_supported() {
    assert_eq!(support(DeviceKind::Camera), Support::Supported);
    assert_eq!(support(DeviceKind::Microphone), Support::Supported);
}
