//! Decode facade errors.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Errors from opening or running a decoder session.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeError {
    /// Codec, output preference, or handle variant is not available.
    #[error("unsupported decode configuration or output")]
    Unsupported,
    /// No platform backend linked / selected for this build.
    #[error("no decode backend available")]
    NoBackend,
    /// Bad dimensions, rates, or packet metadata.
    #[error("invalid decode input")]
    InvalidInput,
    /// Backend rejected the operation (OS/API failure).
    #[error("decoder backend failure")]
    Backend,
    /// Session already finished or not open.
    #[error("decoder session closed")]
    Closed,
}
