//! Crypto errors (`thiserror`).

#![forbid(unsafe_code)]

use thiserror::Error;

/// `ClearKey` / CENC sample crypto error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Error {
    /// Subsample clear+protected lengths exceed the sample buffer.
    #[error("subsample ranges exceed sample length")]
    SubsampleOverflow,
    /// Pattern encryption requested for a scheme that forbids it.
    #[error("pattern is not valid for this protection scheme")]
    InvalidPattern,
    /// IV / key size mismatch for the scheme.
    #[error("invalid key or iv length")]
    InvalidKeyMaterial,
}
