//! Shared device vocabulary: [`DeviceKind`]/[`DeviceId`]/[`Select`], capability/permission
//! probing ([`Support`]/[`Unavailable`]/[`PermissionState`]), the shared [`CaptureError`], and
//! the cross-domain [`DeviceHotplug`] trait.
//!
//! **This crate is the base every capture domain crate depends on — it depends on none of
//! them.** The three domain facades built on top of it:
//!
//! - [`mediaway-device-camera`](https://docs.rs/mediaway-device-camera) — `CameraCapture`.
//! - [`mediaway-device-desktop`](https://docs.rs/mediaway-device-desktop) — `DesktopVideoCapture`
//!   (screen/window) + `DesktopAudioCapture` (loopback/process-loopback — "what's playing").
//! - [`mediaway-device-audio`](https://docs.rs/mediaway-device-audio) — `AudioCapture`
//!   (microphone) + `AudioPlayback` (render endpoint) — "Audio" here means both I/O
//!   directions, not capture alone.
//!
//! See `adr/0007-domain-crate-split.md` for why this crate split from one unified facade.

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
pub mod audio;
pub mod camera;
pub mod desktop;
pub mod linux;
pub mod web;
pub mod windows;
pub mod windows_audio;
pub mod windows_camera;
pub mod windows_desktop;
