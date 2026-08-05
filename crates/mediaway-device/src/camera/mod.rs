//! Camera video capture facade.
//!
//! - **Low-level:** [`CameraCapture`] — OS capture sessions in the platform modules
//!   (`windows_camera`, `linux`).

#![forbid(unsafe_code)]

mod capture;

pub use crate::CaptureError;
pub use capture::{
    CameraCapture, CameraCaptureConfig, CaptureOutputPreference, capture_camera_once,
};

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
