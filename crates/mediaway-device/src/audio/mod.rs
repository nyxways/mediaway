//! Audio I/O facade: microphone capture ([`AudioCapture`]) and render-endpoint playback
//! ([`AudioPlayback`]) — "Audio" here means both I/O directions, not capture alone.
//!
//! Desktop audio (loopback / process-loopback — capturing what the desktop is already
//! rendering) lives in [`crate::desktop`] instead: it is a desktop-capture concept, not a
//! real audio input/output device.
//!
//! - **Low-level:** [`AudioCapture`] / [`AudioPlayback`] — OS sessions in the
//!   `windows_audio` module (and future platform modules).

#![allow(clippy::too_long_first_doc_paragraph)] // crate-root doc became module doc (ADR-0021 merge)
#![forbid(unsafe_code)]

mod capture;
mod error;
mod playback;

pub use crate::CaptureError;
pub use capture::{AudioCapture, AudioCaptureConfig};
pub use error::PlaybackError;
pub use playback::{AudioPlayback, AudioPlaybackConfig};
