//! Windows WASAPI audio I/O backend: microphone capture
//! ([`WindowsWasapiCapture`], implements `crate::audio::AudioCapture`) and
//! render-endpoint playback ([`WindowsWasapiPlayback`], implements
//! `crate::audio::AudioPlayback`).
//!
//! **Also hosts the shared WASAPI capture engine** `mediaway-device-windows-desktop`
//! wraps for loopback/process-loopback capture — [`WindowsWasapiCapture::open`] and the
//! internal [`wasapi_config`] types are `pub` for exactly that reason. See
//! `mediaway-device/adr/0007-domain-crate-split.md`'s "(b)" decision: one shared engine,
//! wrapped per domain, rather than duplicating the capture loop.
//!
//! Split out of `mediaway-device-windows` — same ADR.

#![allow(clippy::too_long_first_doc_paragraph)] // crate-root doc became module doc (ADR-0021 merge)
#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(not(windows), deny(unsafe_code))]

#[cfg(windows)]
mod wasapi;
#[cfg(windows)]
mod wasapi_config;
#[cfg(windows)]
mod wasapi_playback;
#[cfg(windows)]
mod wasapi_process;

#[cfg(windows)]
pub use wasapi::{
    ComGuard, WindowsWasapiCapture, endpoint_friendly_name, endpoint_id, resolve_endpoint,
};
#[cfg(windows)]
pub use wasapi_config::{WasapiCaptureConfig, WasapiProcessTreeScope, WasapiSource};
#[cfg(windows)]
pub use wasapi_playback::WindowsWasapiPlayback;
#[cfg(windows)]
pub use wasapi_process::open_process_loopback_client;

#[cfg(not(windows))]
mod host_stub;
#[cfg(not(windows))]
pub use host_stub::{WindowsWasapiCapture, WindowsWasapiPlayback};

/// Serializes tests that touch real hardware (WASAPI). Mirrors
/// `mediaway-device-windows`'s `HARDWARE_TEST_LOCK` — each split-out backend crate needs its
/// own, since Rust's default test harness runs `#[test]`s concurrently on separate threads.
#[cfg(all(test, windows))]
pub(crate) static HARDWARE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(all(test, windows))]
#[path = "lib_tests.rs"]
mod tests;
