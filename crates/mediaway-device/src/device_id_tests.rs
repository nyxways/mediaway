//! Unit tests for `device_id.rs` (sibling of the implementation).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::{DeviceId, ParseDeviceIdError, Select};
use std::str::FromStr;

#[test]
fn wasapi_display_and_from_str_round_trip() {
    let id = DeviceId::from_wasapi_endpoint_id("{0.0.1.00000000}.{guid}");
    let text = id.to_string();
    assert_eq!(text, "wasapi:{0.0.1.00000000}.{guid}");
    let parsed = DeviceId::from_str(&text).expect("round trip");
    assert_eq!(parsed, id);
    assert_eq!(
        parsed.as_wasapi_endpoint_id(),
        Some("{0.0.1.00000000}.{guid}")
    );
    assert_eq!(parsed.as_media_foundation_symbolic_link(), None);
    assert_eq!(parsed.as_dxgi_output_device_name(), None);
}

#[test]
fn media_foundation_display_and_from_str_round_trip() {
    let id = DeviceId::from_media_foundation_symbolic_link("\\\\?\\usb#vid_1234&pid_5678");
    let text = id.to_string();
    assert_eq!(text, "mf-symlink:\\\\?\\usb#vid_1234&pid_5678");
    let parsed = DeviceId::from_str(&text).expect("round trip");
    assert_eq!(parsed, id);
    assert_eq!(
        parsed.as_media_foundation_symbolic_link(),
        Some("\\\\?\\usb#vid_1234&pid_5678")
    );
    assert_eq!(parsed.as_wasapi_endpoint_id(), None);
}

#[test]
fn dxgi_output_display_and_from_str_round_trip() {
    let id = DeviceId::from_dxgi_output_device_name("\\\\.\\DISPLAY1");
    let text = id.to_string();
    assert_eq!(text, "dxgi-output:\\\\.\\DISPLAY1");
    let parsed = DeviceId::from_str(&text).expect("round trip");
    assert_eq!(parsed, id);
    assert_eq!(parsed.as_dxgi_output_device_name(), Some("\\\\.\\DISPLAY1"));
    assert_eq!(parsed.as_media_foundation_symbolic_link(), None);
}

#[test]
fn pipewire_display_and_from_str_round_trip() {
    let id = DeviceId::from_pipewire_node_name("alsa_input.usb-Blue_Microphones-00.mono-fallback");
    let text = id.to_string();
    assert_eq!(
        text,
        "pipewire:alsa_input.usb-Blue_Microphones-00.mono-fallback"
    );
    let parsed = DeviceId::from_str(&text).expect("round trip");
    assert_eq!(parsed, id);
    assert_eq!(
        parsed.as_pipewire_node_name(),
        Some("alsa_input.usb-Blue_Microphones-00.mono-fallback")
    );
    assert_eq!(parsed.as_wasapi_endpoint_id(), None);
    assert_eq!(parsed.as_media_foundation_symbolic_link(), None);
    assert_eq!(parsed.as_dxgi_output_device_name(), None);
}

#[test]
fn from_str_rejects_unknown_prefix() {
    let err = DeviceId::from_str("bogus:whatever").expect_err("unknown tag prefix");
    assert_eq!(err, ParseDeviceIdError("bogus:whatever".to_owned()));
}

#[test]
fn from_str_rejects_missing_prefix() {
    assert!(DeviceId::from_str("{0.0.1.00000000}.{guid}").is_err());
}

/// Two `DeviceId`s wrapping the same tag+string are equal and hash equal —
/// required for `Select::Id(id)` to be used as a stable lookup/comparison
/// key by callers (ADR-0005).
#[test]
fn equal_ids_compare_equal_across_separate_constructions() {
    let a = DeviceId::from_wasapi_endpoint_id("same-id");
    let b = DeviceId::from_wasapi_endpoint_id("same-id");
    assert_eq!(a, b);
}

/// Same underlying string, different tag — must not compare equal (the tag
/// is real identity, not decoration; see `DeviceIdRepr`'s rustdoc).
#[test]
fn same_string_different_kind_are_not_equal() {
    let wasapi = DeviceId::from_wasapi_endpoint_id("same-value");
    let mf = DeviceId::from_media_foundation_symbolic_link("same-value");
    assert_ne!(wasapi, mf);
}

#[test]
fn select_default_is_the_derive_default() {
    assert_eq!(Select::default(), Select::Default);
}

#[test]
fn select_variants_are_distinguishable() {
    let by_id = Select::Id(DeviceId::from_wasapi_endpoint_id("x"));
    let by_name = Select::NameContains("mic".to_owned());
    assert_ne!(Select::Default, by_id);
    assert_ne!(Select::Default, by_name);
    assert_ne!(by_id, by_name);
}
