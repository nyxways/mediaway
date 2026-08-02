//! Shared capture-session error, common to every capture domain crate
//! (`mediaway-device-camera`/`-desktop`/`-audio`).
//!
//! [`PlaybackError`] moved to `mediaway-device-audio` — playback is Audio
//! I/O-only, unlike [`CaptureError`], which every capture domain shares.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Errors from opening or running a capture session.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaptureError {
    /// Source, output preference, or handle variant is not available.
    #[error("unsupported capture configuration or output")]
    Unsupported,
    /// No platform backend linked / selected for this build.
    #[error("no capture backend available")]
    NoBackend,
    /// Bad dimensions, device pointer, or config.
    #[error("invalid capture input")]
    InvalidInput,
    /// Backend rejected the operation (OS/API failure).
    #[error("capture backend failure")]
    Backend,
    /// Session already finished or not open.
    #[error("capture session closed")]
    Closed,
    /// Desktop duplication / device access denied (secure desktop, ACL, …).
    #[error("capture access denied")]
    AccessDenied,
    /// The device this session was opened against disappeared while live
    /// (unplugged, disabled, or otherwise invalidated). The session is no
    /// longer usable — open a new one.
    #[error("capture device lost")]
    DeviceLost,
    /// A capture trait's `capture_next_frame_blocking`-shaped method's
    /// deadline elapsed with no frame. On an already-open, delta-based
    /// session (e.g. DXGI Desktop Duplication) this can mean "nothing
    /// changed" rather than a backend failure — see that method's docs.
    #[error("capture timed out")]
    Timeout,
}
