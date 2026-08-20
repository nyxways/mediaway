//! Desktop capture facade: screen/window video capture, and desktop audio (loopback /
//! process-loopback — "what's playing") capture.
//!
//! Loopback/process-loopback audio lives here, not in [`crate::audio`]: they capture what
//! the desktop is already rendering (a desktop-capture concept), unlike `audio`'s
//! microphone/render-endpoint "Audio I/O".
//!
//! - **Low-level:** [`DesktopVideoCapture`] / [`DesktopAudioCapture`] — OS capture
//!   sessions in the `windows_desktop` module (and future platform modules).

#![allow(clippy::too_long_first_doc_paragraph)] // crate-root doc became module doc (ADR-0021 merge)
#![forbid(unsafe_code)]

mod audio;
mod video;

pub use crate::CaptureError;
pub use audio::{
    DesktopAudioCapture, DesktopAudioCaptureConfig, DesktopAudioSource, ProcessTreeScope,
};
pub use video::{
    CaptureOutputPreference, CaptureSharing, DesktopCaptureSource, DesktopVideoCapture,
    DesktopVideoCaptureConfig, capture_desktop_video_once,
};
