//! Windows device backend orchestrator: cross-domain capability/permission probing
//! ([`support`]/[`request_permission`]), cross-domain device enumeration ([`enumerate`]),
//! and audio hotplug ([`WindowsDeviceHotplug`]).
//!
//! Per-domain capture backends live in sibling modules: [`crate::windows_camera`],
//! [`crate::windows_desktop`], [`crate::windows_audio`]. This module dispatches across all
//! three (for [`support`]/[`request_permission`]/[`enumerate`]'s cross-`DeviceKind`
//! dispatch) — none of them depend back on this one.
//!
//! - [`WindowsDeviceHotplug::open`] — `IMMNotificationClient` audio hotplug watcher
//!   (Microphone/Loopback only, crate ADR-0005 § Hotplug). Stays here (not one of the
//!   domain modules) because it watches kinds spanning both Audio I/O and Desktop.
//! - [`enumerate`] — live device listing (crate ADR-0005), dispatching by
//!   `DeviceKind` to `crate::windows_camera::enumerate_cameras`/
//!   `crate::windows_desktop::enumerate_outputs`/this module's own WASAPI endpoint
//!   enumeration.
//! - [`support`]/[`request_permission`] — live capability/permission probes
//!   ([crate ADR-0003](../../adr/0003-capability-and-permission-probe.md)).
//!
//! Policy: [ADR-0001](adr/0001-dxgi-desktop-duplication.md), [ADR-0002](adr/0002-wasapi-capture.md),
//! [ADR-0003](adr/0003-capture-exclusion.md), [ADR-0004](adr/0004-wgc-window-capture.md),
//! [ADR-0005](adr/0005-wasapi-playback.md) — predate the [ADR-0021 workspace
//! consolidation](https://github.com/nyxways/mediaway/blob/main/docs/adr/0021-workspace-consolidation.md)
//! that merged the per-domain backends into this crate; superseded in scope, not content.

#![allow(clippy::too_long_first_doc_paragraph)] // crate-root doc became module doc (ADR-0021 merge)
#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(not(windows), deny(unsafe_code))]

#[cfg(windows)]
mod capabilities;
#[cfg(windows)]
mod enumeration;
#[cfg(windows)]
mod gpu;
#[cfg(windows)]
mod hotplug;

#[cfg(windows)]
pub use capabilities::{request_permission, support};
#[cfg(windows)]
pub use enumeration::enumerate;
#[cfg(windows)]
pub use gpu::{
    GpuAdapterInfo, GpuAdapterSelect, GpuDevice, GpuDeviceOptions, enumerate_gpu_adapters,
};
#[cfg(windows)]
pub use hotplug::WindowsDeviceHotplug;

#[cfg(not(windows))]
mod host_stub;
#[cfg(not(windows))]
pub use host_stub::{
    GpuAdapterInfo, GpuAdapterSelect, GpuDevice, GpuDeviceOptions, WindowsDeviceHotplug, enumerate,
    enumerate_gpu_adapters, request_permission, support,
};

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
