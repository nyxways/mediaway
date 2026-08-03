//! Audio playback config and [`AudioPlayback`] trait — the output half of this crate's
//! "Audio I/O" ([`crate::audio::AudioCapture`] is the input half).
//!
//! Moved from `mediaway-device` unchanged — see `mediaway-device/adr/0007-domain-crate-split.md`.

#![forbid(unsafe_code)]

use crate::Select;
use crate::audio::PlaybackError;
use mediaway_common::{AudioFrame, SampleFormat, StreamInfo};

/// Parameters for opening an audio playback session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioPlaybackConfig {
    /// Which render endpoint to open (`Select::Default` = default console
    /// render). Mirrors `crate::desktop::DesktopAudioSource::Loopback { select }`.
    pub select: Select,
    /// Preferred PCM format when conversion is required (`F32` matches modern WASAPI mix).
    pub sample_format: SampleFormat,
}

/// Streaming audio playback — push PCM frames, backend worker thread drains
/// them into the render device on its own schedule.
pub trait AudioPlayback {
    /// Negotiated stream format — real `sample_rate`/`channels`/`format`
    /// after opening (the endpoint's shared-mode mix format on Windows).
    /// `write_frame` payloads must match this exactly; there is no
    /// implicit resample in v1 (see ADR-0004 Deferred).
    fn stream_info(&self) -> &StreamInfo;

    /// Enqueue `frame` for playback (FIFO submission order, no PTS-driven
    /// scheduling in v1 — see Deferred). Ownership transfers on success.
    ///
    /// # Errors
    /// - `PlaybackError::InvalidInput` if `frame`'s format doesn't match
    ///   `stream_info()`.
    /// - `PlaybackError::QueueFull(frame)` if the internal bounded queue is
    ///   full — `frame` is handed back unconsumed (mirrors
    ///   `std::sync::mpsc::SyncSender::try_send`'s `TrySendError<T>`), so the
    ///   caller decides: retry, throttle, or drop. No frame is ever silently
    ///   dropped by the backend on the submission side.
    fn write_frame(&mut self, frame: AudioFrame) -> Result<(), PlaybackError>;

    /// Cumulative count of render periods that included any silence
    /// substituted for missing queued audio, since `open`. Never resets.
    /// A poll-based counter, not an error — playback keeps running through
    /// underrun (see ADR-0004 Buffering), it does not abort.
    fn underrun_count(&self) -> u64;

    /// Stop immediately and free OS resources. **Does not drain** the
    /// internal queue — any buffered-but-unplayed frames are discarded
    /// (blocking `flush`-before-close is a deferred addition, not v1).
    /// Blocks until the backend worker thread has stopped touching the
    /// device (mirrors `WindowsWasapiCapture::close`'s join).
    ///
    /// # Errors
    /// Returns [`PlaybackError`] on backend failure.
    fn close(&mut self) -> Result<(), PlaybackError>;
}
