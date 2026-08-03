//! C ABI status codes (`mediaway_pipeline_status_t`).
//!
//! Fresh, distinctly-named type — not shared or numerically mirrored with
//! `mediaway-container-ffi`'s `MediawayStatus`. See
//! `adr/0001-auto-encode-c-abi.md` §2 for why.

use mediaway::PipelineError;
use mediaway_container::mp4;
use mediaway_encoder::EncodeError;

/// C ABI status code returned by fallible `mediaway-ffi` functions.
///
/// `InvalidArgument`/`HandlePoisoned`/`InternalPanic` are FFI-layer inventions.
/// Everything else maps onto [`EncodeError`] or [`PipelineError`] (both
/// `#[non_exhaustive]`, hence [`Self::UnknownError`] as a catch-all). See
/// `adr/0001-auto-encode-c-abi.md` §2.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayPipelineStatus {
    /// Success.
    Ok = 0,
    /// Null pointer, or mismatched pointer/length pair.
    InvalidArgument = 1,
    /// A previous call already poisoned this handle; the call was refused.
    HandlePoisoned = 2,
    /// [`EncodeError::NoBackend`] — expected/graceful (no backend compiled in).
    NoBackend = 3,
    /// [`EncodeError::Unsupported`] — context-dependent, not always graceful.
    Unsupported = 4,
    /// [`EncodeError::InvalidInput`].
    InvalidInput = 5,
    /// [`EncodeError::Backend`].
    EncoderBackendFailure = 6,
    /// [`EncodeError::Closed`].
    EncoderClosed = 7,
    /// [`mp4::Error::InvalidTrack`], via [`PipelineError::Mux`].
    MuxInvalidTrack = 8,
    /// [`mp4::Error::InvalidPacket`], via [`PipelineError::Mux`].
    MuxInvalidPacket = 9,
    /// [`mp4::Error::InvalidData`], via [`PipelineError::Mux`].
    MuxInvalidData = 10,
    /// A future `#[non_exhaustive]` variant on `EncodeError`, `mp4::Error`, or
    /// `PipelineError` itself.
    UnknownError = 11,
    /// This call caught a Rust panic; the handle is now poisoned.
    InternalPanic = 12,
}

impl From<EncodeError> for MediawayPipelineStatus {
    fn from(err: EncodeError) -> Self {
        match err {
            EncodeError::NoBackend => Self::NoBackend,
            EncodeError::Unsupported => Self::Unsupported,
            EncodeError::InvalidInput => Self::InvalidInput,
            EncodeError::Backend => Self::EncoderBackendFailure,
            EncodeError::Closed => Self::EncoderClosed,
            _ => Self::UnknownError,
        }
    }
}

impl From<mp4::Error> for MediawayPipelineStatus {
    fn from(err: mp4::Error) -> Self {
        match err {
            mp4::Error::InvalidTrack => Self::MuxInvalidTrack,
            mp4::Error::InvalidPacket => Self::MuxInvalidPacket,
            mp4::Error::InvalidData => Self::MuxInvalidData,
            _ => Self::UnknownError,
        }
    }
}

impl From<PipelineError> for MediawayPipelineStatus {
    fn from(err: PipelineError) -> Self {
        match err {
            PipelineError::Encode(e) => e.into(),
            PipelineError::Mux(e) => e.into(),
            _ => Self::UnknownError,
        }
    }
}
