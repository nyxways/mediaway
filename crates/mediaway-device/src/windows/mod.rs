//! Windows device backend orchestrator: cross-domain capability/permission probing
//! ([`support`]/[`request_permission`]), cross-domain device enumeration ([`enumerate`]),
//! and audio hotplug ([`WindowsDeviceHotplug`]).
//!
//! **Split out of the original unified `mediaway-device-windows`** — see
//! `mediaway-device/adr/0007-domain-crate-split.md`. Per-domain capture backends now live
//! in their own crates: `mediaway-device-windows-camera`, `mediaway-device-windows-desktop`,
//! `mediaway-device-windows-audio`. This crate depends on all three (for [`support`]/
//! [`request_permission`]/[`enumerate`]'s cross-`DeviceKind` dispatch) — none of them depend
//! back on this one, so there is no cycle.
//!
//! - [`WindowsDeviceHotplug::open`] — `IMMNotificationClient` audio hotplug watcher
//!   (Microphone/Loopback only, `mediaway-device` ADR-0005 § Hotplug). Stays here (not one
//!   of the domain crates) because it watches kinds spanning both Audio I/O and Desktop.
//! - [`enumerate`] — live device listing (`mediaway-device` ADR-0005), dispatching by
//!   `DeviceKind` to `crate::windows_camera::enumerate_cameras`/
//!   `crate::windows_desktop::enumerate_outputs`/this crate's own WASAPI endpoint
//!   enumeration.
//! - [`support`]/[`request_permission`] — live capability/permission probes
//!   ([`mediaway-device` ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md)).
//!
//! Policy: [ADR-0001](adr/0001-dxgi-desktop-duplication.md), [ADR-0002](adr/0002-wasapi-capture.md),
//! [ADR-0003](adr/0003-capture-exclusion.md), [ADR-0004](adr/0004-wgc-window-capture.md),
//! [ADR-0005](adr/0005-wasapi-playback.md) — historical, from before this crate split;
//! superseded in scope (not content) by the crates named above.

#![allow(clippy::too_long_first_doc_paragraph)] // crate-root doc became module doc (ADR-0021 merge)
#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(not(windows), deny(unsafe_code))]

#[cfg(windows)]
mod capabilities;
#[cfg(windows)]
mod enumeration;
#[cfg(windows)]
mod hotplug;

#[cfg(windows)]
pub use capabilities::{request_permission, support};
#[cfg(windows)]
pub use enumeration::enumerate;
#[cfg(windows)]
pub use hotplug::WindowsDeviceHotplug;

#[cfg(not(windows))]
mod host_stub;
#[cfg(not(windows))]
pub use host_stub::{WindowsDeviceHotplug, enumerate, request_permission, support};

/// Serializes tests that touch real hardware (WASAPI hotplug/`MMDeviceEnumerator`) across
/// `lib_tests.rs`/`capabilities_tests.rs`/`enumeration_tests.rs`. Rust's default test
/// harness runs `#[test]`s concurrently on separate threads; concurrent real WASAPI
/// sessions from this crate's `_or_skip` tests reproduced a genuine `STATUS_ACCESS_VIOLATION`
/// crash before this lock was added — this is not a hypothetical concern.
#[cfg(all(test, windows))]
pub(crate) static HARDWARE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(all(test, windows))]
#[path = "lib_tests.rs"]
mod tests;
