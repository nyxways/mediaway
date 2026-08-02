//! Shared encode errors.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Errors from opening or running an encoder session.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeError {
    /// Requested codec, pixel format, or handle variant is not available.
    #[error("unsupported encode configuration or input")]
    Unsupported,
    /// No platform backend linked / selected for this build.
    #[error("no encode backend available")]
    NoBackend,
    /// Bad dimensions, rates, or frame metadata.
    #[error("invalid encode input")]
    InvalidInput,
    /// Backend rejected the operation (OS/API failure). Details in logs when available.
    #[error("encoder backend failure")]
    Backend,
    /// Session already finished or not open.
    #[error("encoder session closed")]
    Closed,
}
