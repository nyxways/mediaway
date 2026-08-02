//! Audio I/O facade: microphone capture ([`AudioCapture`]) and render-endpoint playback
//! ([`AudioPlayback`]) — "Audio" here means both I/O directions, not capture alone. Split
//! out of `mediaway-device`'s former unified facade — see
//! `mediaway-device/adr/0007-domain-crate-split.md`.
//!
//! Desktop audio (loopback / process-loopback — capturing what the desktop is already
//! rendering) lives in `mediaway-device-desktop` instead: it is a desktop-capture
//! concept, not a real audio input/output device.
//!
//! - **Low-level:** [`AudioCapture`] / [`AudioPlayback`] — OS sessions in
//!   `mediaway-device-windows-audio` (and future platform crates).

#![forbid(unsafe_code)]

mod capture;
mod error;
mod playback;

pub use capture::{AudioCapture, AudioCaptureConfig};
pub use error::PlaybackError;
pub use mediaway_device::CaptureError;
pub use playback::{AudioPlayback, AudioPlaybackConfig};
