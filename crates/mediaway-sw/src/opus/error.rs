//! [`OpusError`] — errors from [`crate::opus::OpusEncoder`] / [`crate::opus::OpusDecoder`] sessions.

use thiserror::Error;

/// Errors from opening or running an [`crate::opus::OpusEncoder`] / [`crate::opus::OpusDecoder`] session.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpusError {
    /// `unsafe-libopus` rejected a call. `code` is the raw libopus error code
    /// (negative; see `unsafe_libopus::OPUS_*` constants), `message` is the
    /// human-readable string from the dependency's own safe `opus_strerror`.
    #[error("libopus error {code}: {message}")]
    Backend {
        /// Raw libopus error code.
        code: i32,
        /// Human-readable message from `unsafe_libopus::opus_strerror`.
        message: &'static str,
    },
    /// Input `AudioFrame`/output PCM is not [`mediaway_common::SampleFormat::F32`] —
    /// the only format this crate accepts (`opus_encode_float` / `opus_decode_float`).
    #[error("unsupported sample format: mediaway-sw-opus only accepts SampleFormat::F32")]
    UnsupportedSampleFormat,
    /// `AudioFrame::sample_rate` / `channels` does not match the session's
    /// configured values.
    #[error("audio frame sample_rate/channels does not match the session's configuration")]
    ConfigMismatch,
    /// `AudioFrame::data` length does not match the session's fixed Opus
    /// frame size (derived from `sample_rate` and `time_base` at `open`).
    /// Opus requires PCM input in exact legal frame durations; this crate
    /// never re-buffers/re-chunks — callers must submit exactly
    /// `expected_bytes` per call.
    #[error(
        "PCM frame size mismatch: session expects {expected_samples} samples/channel \
         ({expected_bytes} bytes), got {actual_bytes} bytes"
    )]
    FrameSizeMismatch {
        /// Expected samples per channel (the session's configured Opus frame size).
        expected_samples: usize,
        /// Expected byte length (`expected_samples * channels * size_of::<f32>()`).
        expected_bytes: usize,
        /// Actual buffer length received.
        actual_bytes: usize,
    },
    /// `time_base` does not evenly divide into a whole PCM sample count at
    /// `sample_rate` — Opus frames must be an exact number of samples.
    #[error(
        "invalid frame duration: time_base {num}/{den} does not divide evenly into a whole \
         sample count at {sample_rate} Hz"
    )]
    InvalidFrameDuration {
        /// Configured `time_base` numerator.
        num: u64,
        /// Configured `time_base` denominator.
        den: u32,
        /// Configured sample rate (Hz).
        sample_rate: u32,
    },
    /// Session already flushed; no further input is accepted.
    #[error("Opus session closed")]
    Closed,
}
