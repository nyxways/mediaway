//! Desktop capture facade: screen/window video capture, and desktop audio (loopback /
//! process-loopback — "what's playing") capture. Split out of `mediaway-device`'s former
//! unified facade — see `mediaway-device/adr/0007-domain-crate-split.md`.
//!
//! Loopback/process-loopback audio lives here, not in `mediaway-device-audio`: they
//! capture what the desktop is already rendering (a desktop-capture concept), unlike
//! `mediaway-device-audio`'s microphone/render-endpoint "Audio I/O".
//!
//! - **Low-level:** [`DesktopVideoCapture`] / [`DesktopAudioCapture`] — OS capture
//!   sessions in `mediaway-device-windows-desktop` (and future platform crates).

#![forbid(unsafe_code)]

mod audio;
mod video;

pub use audio::{
    DesktopAudioCapture, DesktopAudioCaptureConfig, DesktopAudioSource, ProcessTreeScope,
};
pub use mediaway_device::CaptureError;
pub use video::{
    CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCapture, DesktopVideoCaptureConfig,
    capture_desktop_video_once,
};
