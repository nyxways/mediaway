//! Camera video capture facade — split out of `mediaway-device`'s former unified video
//! capture surface. See `mediaway-device/adr/0007-domain-crate-split.md`.
//!
//! - **Low-level:** [`CameraCapture`] — OS capture sessions in `mediaway-device-<platform>`.

#![forbid(unsafe_code)]

mod capture;

pub use capture::{
    CameraCapture, CameraCaptureConfig, CaptureOutputPreference, capture_camera_once,
};
pub use mediaway_device::CaptureError;

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
