//! Windows desktop capture backend: DXGI Desktop Duplication screen capture
//! ([`WindowsScreenCapture`]), Windows Graphics Capture window capture
//! ([`WindowsWindowCapture`]) — both implement `crate::desktop::DesktopVideoCapture`
//! — and desktop audio (loopback / process-loopback) via
//! [`WindowsDesktopAudioCapture`] — implements `crate::desktop::DesktopAudioCapture`
//! by wrapping [`crate::windows_audio`]'s shared WASAPI engine.

#![allow(clippy::too_long_first_doc_paragraph)] // crate-root doc became module doc (ADR-0021 merge)
#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(not(windows), deny(unsafe_code))]

#[cfg(all(windows, feature = "desktop"))]
mod canvas;
#[cfg(all(windows, feature = "desktop"))]
mod capture_exclusion;
#[cfg(all(windows, feature = "desktop"))]
mod desktop_audio;
#[cfg(all(windows, feature = "desktop"))]
mod dxgi;
#[cfg(all(windows, feature = "desktop"))]
mod dxgi_exclusive;
#[cfg(all(windows, feature = "desktop"))]
mod dxgi_shared;
#[cfg(all(windows, feature = "desktop"))]
mod wgc;

mod options;
pub use options::{CaptureBorder, FrameDimensions, WindowCaptureOptions};

#[cfg(all(windows, feature = "desktop"))]
pub use canvas::{Canvas, CanvasFrame, CanvasPlacement};
#[cfg(all(windows, feature = "desktop"))]
pub use capture_exclusion::exclude_window_from_capture;
#[cfg(all(windows, feature = "desktop"))]
pub use desktop_audio::WindowsDesktopAudioCapture;
#[cfg(all(windows, feature = "desktop"))]
pub use dxgi::{WindowsScreenCapture, enumerate_outputs};
#[cfg(all(windows, feature = "desktop"))]
pub use wgc::WindowsWindowCapture;

#[cfg(not(all(windows, feature = "desktop")))]
mod host_stub;
#[cfg(not(all(windows, feature = "desktop")))]
pub use host_stub::{
    WindowsDesktopAudioCapture, WindowsScreenCapture, WindowsWindowCapture, enumerate_outputs,
    exclude_window_from_capture,
};

/// Serializes tests that touch real hardware (DXGI/WGC). Mirrors
/// [`crate::windows`]'s `HARDWARE_TEST_LOCK` — each hardware-touching module needs its own,
/// since Rust's default test harness runs `#[test]`s concurrently on separate threads.
#[cfg(all(test, windows, feature = "desktop"))]
pub(crate) static HARDWARE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(all(test, windows, feature = "desktop"))]
#[path = "lib_tests.rs"]
mod tests;
