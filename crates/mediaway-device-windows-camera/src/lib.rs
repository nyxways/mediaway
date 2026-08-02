//! Windows camera capture backend (Media Foundation `IMFSourceReader`, CPU copy). Implements
//! `mediaway_device_camera::CameraCapture`. Split out of `mediaway-device-windows` — see
//! `mediaway-device/adr/0007-domain-crate-split.md`.

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
/// `mediaway-device-windows`'s `HARDWARE_TEST_LOCK` — each split-out backend crate needs its
/// own, since Rust's default test harness runs `#[test]`s concurrently on separate threads.
#[cfg(all(test, windows))]
pub(crate) static HARDWARE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
