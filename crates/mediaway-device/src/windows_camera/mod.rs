//! Windows camera capture backend (Media Foundation `IMFSourceReader`, CPU copy). Implements
//! `crate::camera::CameraCapture`.

#![allow(clippy::too_long_first_doc_paragraph)] // crate-root doc became module doc (ADR-0021 merge)
#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(not(windows), deny(unsafe_code))]

#[cfg(all(windows, feature = "camera"))]
mod capture;

#[cfg(all(windows, feature = "camera"))]
pub use capture::{WindowsCameraCapture, enumerate_cameras};

#[cfg(not(all(windows, feature = "camera")))]
mod host_stub;
#[cfg(not(all(windows, feature = "camera")))]
pub use host_stub::{WindowsCameraCapture, enumerate_cameras};

/// Serializes tests that touch real hardware (Media Foundation). Mirrors
/// [`crate::windows`]'s `HARDWARE_TEST_LOCK` — each hardware-touching module needs its own,
/// since Rust's default test harness runs `#[test]`s concurrently on separate threads.
#[cfg(all(test, windows, feature = "camera"))]
pub(crate) static HARDWARE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
