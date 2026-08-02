//! Audio playback errors — moved from `mediaway-device` (playback is Audio I/O-only,
//! unlike `mediaway_device::CaptureError`, which every capture domain shares). See
//! `mediaway-device/adr/0007-domain-crate-split.md`.

#![forbid(unsafe_code)]

use mediaway_common::AudioFrame;
use thiserror::Error;

/// Errors from opening or running a playback session.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlaybackError {
    /// Device index, output preference, or handle variant is not available.
    #[error("unsupported playback configuration or output")]
    Unsupported,
    /// No platform backend linked / selected for this build.
    #[error("no playback backend available")]
    NoBackend,
    /// `AudioFrame` format doesn't match the negotiated `stream_info()`.
    #[error("invalid playback input")]
    InvalidInput,
    /// Backend rejected the operation (OS/API failure).
    #[error("playback backend failure")]
    Backend,
    /// Session already finished or not open.
    #[error("playback session closed")]
    Closed,
    /// Render device access denied.
    #[error("playback access denied")]
    AccessDenied,
    /// Internal bounded queue is full; `frame` is handed back unconsumed.
    #[error("playback queue full")]
    QueueFull(AudioFrame),
    /// The device this session was opened against disappeared while live
    /// (unplugged, disabled, or otherwise invalidated). The session is no
    /// longer usable — open a new one.
    #[error("playback device lost")]
    DeviceLost,
}
