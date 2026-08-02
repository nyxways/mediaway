//! Linux capture backend (`xdg-desktop-portal` `ScreenCast` + `PipeWire` +
//! `V4L2`).
//!
//! - [`LinuxScreenCapture::open`] — portal-mediated screen capture, **CPU copy**
//!   path (`Bgra8` / `Rgba8` / `I420` depending on negotiation, never GPU
//!   Zero-Copy this session). See
//!   [ADR-0001](adr/0001-portal-pipewire-screen-capture.md).
//! - [`LinuxCameraCapture::open`] — `V4L2` `mmap` streaming I/O (`YUYV` /
//!   `NV12` / `I420`, CPU frames). See
//!   [ADR-0002](adr/0002-v4l2-camera-capture.md).
//! - [`LinuxWindowCapture::open`] — the same portal `ScreenCast` + `PipeWire`
//!   plumbing as screen capture, requesting `SourceType::Window`. See
//!   [ADR-0003](adr/0003-portal-window-capture.md).
//! - [`LinuxMicrophoneCapture::open`] — a direct `PipeWire` audio stream (no
//!   portal — normal desktop `PipeWire` clients need none for audio). See
//!   [ADR-0004](adr/0004-pipewire-microphone-capture.md).
//!
//! **Zero runtime hardware/session verification happened in this development
//! session** for any of the four backends above. No real desktop portal
//! session, `PipeWire` stream/daemon, V4L2 device, or compositor was
//! exercised — see each ADR's § Zero runtime verification. Compare
//! `mediaway-device-windows`, whose DXGI/WGC/WASAPI backends *did* get real
//! hardware verification in earlier sessions.

#![cfg_attr(target_os = "linux", allow(unsafe_code))]
#![cfg_attr(not(target_os = "linux"), deny(unsafe_code))]
// `format`/`portal` are private modules (`mod format;`, not `pub mod`), so items
// they expose to `screencast.rs` are only ever crate-reachable either way.
// `pub(crate)` documents that intent and satisfies `unreachable_pub`; clippy's
// `redundant_pub_crate` (nursery) disagrees and asks for plain `pub` instead,
// which would reintroduce `unreachable_pub` warnings. Keep `pub(crate)`.
#![cfg_attr(target_os = "linux", allow(clippy::redundant_pub_crate))]

#[cfg(target_os = "linux")]
mod camera;
#[cfg(target_os = "linux")]
mod capabilities;
#[cfg(target_os = "linux")]
mod format;
#[cfg(target_os = "linux")]
mod mic;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
mod screencast;
#[cfg(target_os = "linux")]
mod window;

#[cfg(target_os = "linux")]
pub use camera::LinuxCameraCapture;
#[cfg(target_os = "linux")]
pub use capabilities::{request_permission, support};
#[cfg(target_os = "linux")]
pub use mic::LinuxMicrophoneCapture;
#[cfg(target_os = "linux")]
pub use screencast::LinuxScreenCapture;
#[cfg(target_os = "linux")]
pub use window::LinuxWindowCapture;

#[cfg(not(target_os = "linux"))]
mod host_stub;
#[cfg(not(target_os = "linux"))]
pub use host_stub::{
    LinuxCameraCapture, LinuxMicrophoneCapture, LinuxScreenCapture, LinuxWindowCapture,
    request_permission, support,
};
