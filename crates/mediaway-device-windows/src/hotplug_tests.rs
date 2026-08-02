//! Unit tests for `hotplug.rs` (sibling of the implementation).
//!
//! `map_dataflow_to_kind`/`map_default_changed_kind` are pure `EDataFlow`/
//! `ERole` → `DeviceKind` mapping logic — no COM/`unsafe` — exercised
//! directly here without live hardware, mirroring `wasapi.rs`'s
//! `combine_endpoint_and_interface_names` extraction pattern.
//! `push_bounded`'s drop-oldest behavior is exercised against a bare
//! `HotplugQueue`, no `WindowsDeviceHotplug` session needed.
//! `WindowsDeviceHotplug::open`'s input-validation rejections (empty
//! `kinds` / an out-of-v1-scope `DeviceKind`) fail before any COM call, so
//! they are also safe to run here without `HARDWARE_TEST_LOCK` — the real
//! `IMMNotificationClient` registration path is covered by a
//! hardware-gated test in `lib_tests.rs` instead.

#![cfg(test)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use std::collections::VecDeque;
use std::sync::Mutex;

use windows::Win32::Media::Audio::{
    eAll, eCapture, eCommunications, eConsole, eMultimedia, eRender,
};

use super::*;

fn queue() -> HotplugQueue {
    HotplugQueue {
        events: Mutex::new(VecDeque::new()),
    }
}

fn added(tag: &str) -> DeviceEvent {
    DeviceEvent::Added {
        id: DeviceId::from_wasapi_endpoint_id(tag),
        kind: DeviceKind::Microphone,
    }
}

#[test]
fn map_dataflow_to_kind_covers_capture_and_render() {
    assert_eq!(map_dataflow_to_kind(eCapture), Some(DeviceKind::Microphone));
    assert_eq!(map_dataflow_to_kind(eRender), Some(DeviceKind::Loopback));
    assert_eq!(map_dataflow_to_kind(eAll), None);
}

#[test]
fn map_default_changed_kind_filters_to_console_role() {
    assert_eq!(
        map_default_changed_kind(eCapture, eConsole),
        Some(DeviceKind::Microphone)
    );
    assert_eq!(
        map_default_changed_kind(eRender, eConsole),
        Some(DeviceKind::Loopback)
    );
    // Non-console roles are real OS notifications but ADR-0005's "eConsole
    // is the default role" precedent means they must not turn into a
    // second/third `DefaultChanged` event for the same underlying switch.
    assert_eq!(map_default_changed_kind(eCapture, eMultimedia), None);
    assert_eq!(map_default_changed_kind(eRender, eCommunications), None);
    assert_eq!(map_default_changed_kind(eAll, eConsole), None);
}

#[test]
#[allow(
    clippy::significant_drop_tightening,
    reason = "single-threaded test; the lock guard is held for the rest of this \
              function's short body on purpose, to assert against one consistent snapshot"
)]
fn push_bounded_keeps_fifo_order_under_capacity() {
    let q = queue();
    push_bounded(&q, added("a"));
    push_bounded(&q, added("b"));
    let mut events = q.events.lock().unwrap();
    assert_eq!(events.pop_front(), Some(added("a")));
    assert_eq!(events.pop_front(), Some(added("b")));
    assert_eq!(events.pop_front(), None);
}

#[test]
#[allow(
    clippy::significant_drop_tightening,
    reason = "single-threaded test; the lock guard is held for the rest of this \
              function's short body on purpose, to assert against one consistent snapshot"
)]
fn push_bounded_drops_oldest_past_capacity() {
    let q = queue();
    for i in 0..HOTPLUG_QUEUE_CAP + 2 {
        push_bounded(&q, added(&i.to_string()));
    }
    let events = q.events.lock().unwrap();
    assert_eq!(events.len(), HOTPLUG_QUEUE_CAP);
    // The two oldest ("0", "1") were dropped; the front is now "2".
    assert_eq!(events.front(), Some(&added("2")));
    assert_eq!(
        events.back(),
        Some(&added(&(HOTPLUG_QUEUE_CAP + 1).to_string()))
    );
}

#[test]
fn open_rejects_empty_kinds() {
    assert!(matches!(
        WindowsDeviceHotplug::open(&[]),
        Err(CaptureError::InvalidInput)
    ));
}

#[test]
fn open_rejects_out_of_v1_scope_kind() {
    for kind in [
        DeviceKind::Camera,
        DeviceKind::Screen,
        DeviceKind::Window,
        DeviceKind::ProcessLoopback,
    ] {
        assert!(
            matches!(
                WindowsDeviceHotplug::open(&[kind]),
                Err(CaptureError::Unsupported)
            ),
            "expected Unsupported for {kind:?}"
        );
    }
}

#[test]
fn open_rejects_mixed_in_scope_and_out_of_scope_kinds() {
    assert!(matches!(
        WindowsDeviceHotplug::open(&[DeviceKind::Microphone, DeviceKind::Camera]),
        Err(CaptureError::Unsupported)
    ));
}
