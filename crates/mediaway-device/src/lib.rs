//! Device capture facade: shared vocabulary ([`DeviceKind`]/[`DeviceId`]/[`Select`]),
//! capability/permission probing ([`Support`]/[`Unavailable`]/[`PermissionState`]), the shared
//! [`CaptureError`], and the cross-domain [`DeviceHotplug`] trait — plus three domain modules
//! built on top of them:
//!
//! - [`camera`] — `CameraCapture`.
//! - [`desktop`] — `DesktopVideoCapture` (screen/window) + `DesktopAudioCapture`
//!   (loopback/process-loopback — "what's playing").
//! - [`audio`] — `AudioCapture` (microphone) + `AudioPlayback` (render endpoint) — "Audio"
//!   here means both I/O directions, not capture alone.
//!
//! Platform backends (`android`, `apple`, `windows`, `windows_audio`, `windows_camera`,
//! `windows_desktop`, `linux`, `web`) are `#[cfg]`-gated modules in this same crate rather than
//! separate crates — see
//! [ADR-0021](https://github.com/nyxways/mediaway/blob/main/docs/adr/0021-workspace-consolidation.md).

#![allow(unsafe_code)]

mod capability;
mod device_id;
mod enumeration;
mod error;
mod hotplug;

pub use capability::{DeviceKind, PermissionState, Support, Unavailable};
pub use device_id::{DeviceId, ParseDeviceIdError, Select};
pub use enumeration::DeviceInfo;
pub use error::CaptureError;
pub use hotplug::{DeviceEvent, DeviceHotplug};

// ── merged platform/domain modules (ADR-0021) ──
pub mod android;
pub mod apple;
pub mod audio;
pub mod camera;
pub mod desktop;
pub mod linux;
pub mod web;
pub mod windows;
pub mod windows_audio;
pub mod windows_camera;
pub mod windows_desktop;
