//! Windows camera capture backend (Media Foundation `IMFSourceReader`, CPU copy). Implements
//! `crate::camera::CameraCapture`.

#![allow(clippy::too_long_first_doc_paragraph)] // crate-root doc became module doc (ADR-0021 merge)
#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(not(windows), deny(unsafe_code))]

#[cfg(windows)]
mod capture;

#[cfg(windows)]
pub use capture::{WindowsCameraCapture, enumerate_cameras};

#[cfg(not(windows))]
mod host_stub;
#[cfg(not(windows))]
pub use host_stub::{WindowsCameraCapture, enumerate_cameras};

/// Serializes tests that touch real hardware (Media Foundation). Mirrors
/// [`crate::windows`]'s `HARDWARE_TEST_LOCK` — each hardware-touching module needs its own,
/// since Rust's default test harness runs `#[test]`s concurrently on separate threads.
#[cfg(all(test, windows))]
pub(crate) static HARDWARE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
