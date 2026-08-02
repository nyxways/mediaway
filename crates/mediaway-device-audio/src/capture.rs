//! Microphone capture config and [`AudioCapture`] trait — the input half of this
//! crate's "Audio I/O" ([`crate::AudioPlayback`] is the output half).
//!
//! Split out of `mediaway-device`'s former unified `audio.rs` — see
//! `mediaway-device/adr/0007-domain-crate-split.md`. Narrowed to microphone only:
//! loopback/process-loopback ("what's playing") moved to `mediaway-device-desktop`,
//! since those capture desktop output, not a real audio input device.

#![forbid(unsafe_code)]

use mediaway_common::{AudioFrame, Rational, SampleFormat, StreamInfo};
use mediaway_device::{CaptureError, Select};

/// Parameters for opening a microphone capture session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioCaptureConfig {
    /// Which capture endpoint to open (`Select::Default` = default console capture).
    pub select: Select,
    /// Timestamp timebase for polled frames (often `1 / sample_rate`).
    pub time_base: Rational,
    /// Preferred PCM format when conversion is required (`F32` matches modern WASAPI mix).
    pub sample_format: SampleFormat,
}

impl AudioCaptureConfig {
    /// Default microphone capture. Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn microphone(time_base: Rational) -> Self {
        Self {
            select: Select::Default,
            time_base,
            sample_format: SampleFormat::F32,
        }
    }
}

/// Streaming microphone capture — poll PCM frames (worker may fill a bounded queue).
pub trait AudioCapture {
    /// Stream metadata — `StreamInfo::Audio` with real `sample_rate`/
    /// `channels`, `codec: CodecKind::RawAudio` for uncompressed PCM.
    fn stream_info(&self) -> &StreamInfo;

    /// Pull the next PCM chunk if ready. `Ok(None)` = no samples yet.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError>;

    /// End the session and free OS resources.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    fn close(&mut self) -> Result<(), CaptureError>;
}
