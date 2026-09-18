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
    /// Same failure class as [`Self::Backend`], but the platform named a status
    /// code: a Windows `HRESULT`, a POSIX `errno`, … Backends that cannot produce
    /// one (a JNI exception, for instance) keep returning [`Self::Backend`].
    ///
    /// Carrying the code is what lets a caller — or a test — distinguish
    /// "this machine does not support it" from "we called the API wrong", which a
    /// bare [`Self::Backend`] cannot. Shape mirrors `mediaway_sw::opus::OpusError::Backend`.
    #[error("capture backend failure (native code {code:#010x})")]
    BackendCode {
        /// Raw platform status code, stored with its original bit pattern
        /// (`HRESULT` is already `i32`; other platforms sign-extend into one).
        code: i32,
    },
    /// The requested [`crate::desktop::CaptureRegion`] does not fit the captured surface —
    /// at open, or later because the source shrank. Never padded instead: a padded frame
    /// holds pixels the source did not draw.
    #[error(
        "capture region {width}x{height} at ({x}, {y}) does not fit a \
         {surface_width}x{surface_height} surface"
    )]
    RegionOutOfBounds {
        /// Region left edge.
        x: u32,
        /// Region top edge.
        y: u32,
        /// Region width.
        width: u32,
        /// Region height.
        height: u32,
        /// Width of the surface it had to fit.
        surface_width: u32,
        /// Height of the surface it had to fit.
        surface_height: u32,
    },
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
