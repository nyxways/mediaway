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

// Backend families (crate features `audio` / `camera` / `desktop`): a disabled family is replaced
// by the same `host_stub` the non-Linux hosts use, and its native dependency (PipeWire / V4L2 /
// the portal client) is not even built. `capabilities` is always real on Linux and reports a
// disabled family as `NotImplemented` (see its `cfg` branches).
#[cfg(all(target_os = "linux", feature = "camera"))]
mod camera;
#[cfg(target_os = "linux")]
mod capabilities;
#[cfg(all(target_os = "linux", feature = "desktop"))]
mod format;
#[cfg(all(target_os = "linux", feature = "audio"))]
mod mic;
#[cfg(all(target_os = "linux", feature = "desktop"))]
mod portal;
#[cfg(all(target_os = "linux", feature = "desktop"))]
mod screencast;
#[cfg(all(target_os = "linux", feature = "desktop"))]
mod window;

#[cfg(all(target_os = "linux", feature = "camera"))]
pub use camera::LinuxCameraCapture;
#[cfg(target_os = "linux")]
pub use capabilities::{request_permission, support};
#[cfg(all(target_os = "linux", feature = "audio"))]
pub use mic::LinuxMicrophoneCapture;
#[cfg(all(target_os = "linux", feature = "desktop"))]
pub use screencast::LinuxScreenCapture;
#[cfg(all(target_os = "linux", feature = "desktop"))]
pub use window::LinuxWindowCapture;

#[cfg(any(
    not(target_os = "linux"),
    not(feature = "audio"),
    not(feature = "camera"),
    not(feature = "desktop")
))]
mod host_stub;
#[cfg(not(all(target_os = "linux", feature = "camera")))]
pub use host_stub::LinuxCameraCapture;
#[cfg(not(all(target_os = "linux", feature = "audio")))]
pub use host_stub::LinuxMicrophoneCapture;
#[cfg(not(all(target_os = "linux", feature = "desktop")))]
pub use host_stub::{LinuxScreenCapture, LinuxWindowCapture};
#[cfg(not(target_os = "linux"))]
pub use host_stub::{request_permission, support};
