//! Errors returned by [`AudioProcessor`](crate::AudioProcessor) and
//! [`VoiceActivityDetector`](crate::VoiceActivityDetector).

use mediaway_common::SampleFormat;
use thiserror::Error;

/// Errors returned by this crate's `sonora`-backed audio types.
///
/// [`BackendPanicked`](ApmError::BackendPanicked) is more than an error
/// value — it is a signal that the instance which returned it has been
/// permanently disabled after `sonora`/`sonora-agc2` panicked internally
/// (see this crate's panic-safety posture in
/// `adr/0001-sonora-audio-processing-adoption.md` § 4). Check
/// `is_disabled()` on the relevant type to understand what happens next:
/// [`AudioProcessor`](crate::AudioProcessor) passes raw PCM through
/// unmodified on every later call; [`VoiceActivityDetector`](crate::VoiceActivityDetector)
/// has no honest passthrough for a scalar score and instead returns this
/// same error on every later call — see its `analyze` rustdoc.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ApmError {
    /// `sonora` processes `f32` PCM only; some other [`SampleFormat`] was
    /// supplied — either at construction ([`AudioProcessor::open`](crate::AudioProcessor::open))
    /// or on a pushed/analyzed [`AudioFrame`](mediaway_common::AudioFrame).
    #[error("unsupported sample format {0:?} — this crate requires SampleFormat::F32")]
    UnsupportedSampleFormat(SampleFormat),

    /// A pushed [`AudioFrame`](mediaway_common::AudioFrame)'s sample rate or
    /// channel count does not match the stream format the
    /// [`AudioProcessor`](crate::AudioProcessor) instance was opened with.
    #[error(
        "frame format ({actual_sample_rate} Hz, {actual_channels} ch) does not match the \
         stream this instance was opened with ({expected_sample_rate} Hz, {expected_channels} ch)"
    )]
    StreamFormatMismatch {
        /// Sample rate the instance was configured for.
        expected_sample_rate: u32,
        /// Channel count the instance was configured for.
        expected_channels: u16,
        /// Sample rate the offending frame actually carried.
        actual_sample_rate: u32,
        /// Channel count the offending frame actually carried.
        actual_channels: u16,
    },

    /// [`VoiceActivityDetector::analyze`](crate::VoiceActivityDetector::analyze)
    /// requires an exact 10ms block (`sample_rate / 100` samples per
    /// channel) — no internal re-blocking ring buffer exists (by design; see
    /// that method's rustdoc).
    #[error(
        "frame carries {actual} samples per channel, expected exactly {expected} (one 10ms block)"
    )]
    FrameLengthMismatch {
        /// Expected samples per channel (`sample_rate / 100`).
        expected: usize,
        /// Samples per channel the frame actually carried.
        actual: usize,
    },

    /// `sonora`/`sonora-agc2` panicked inside a `catch_unwind`-wrapped call
    /// (`build`, `process_render_f32`, `process_capture_f32`, or
    /// `analyze`). The instance that returned this is now permanently
    /// disabled — see the enum-level docs and `is_disabled()` on the
    /// relevant type.
    #[error("sonora backend panicked and has been disabled for this instance")]
    BackendPanicked,

    /// `sonora::AudioProcessing::process_capture_f32`/`process_render_f32`
    /// returned a typed (non-panic) error — e.g. an unsupported sample rate
    /// or a channel-count mismatch. Does **not** disable the instance; only
    /// [`BackendPanicked`](Self::BackendPanicked) does.
    #[cfg(feature = "apm")]
    #[error("sonora audio processing rejected the stream: {0}")]
    Backend(#[source] sonora::Error),
}
