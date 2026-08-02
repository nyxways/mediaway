//! CLI error type — maps to an exit code and an English stderr message.

use std::io;
use thiserror::Error;

/// Top-level probe CLI error.
#[derive(Debug, Error)]
pub(crate) enum ProbeError {
    /// Bad or missing command-line arguments.
    #[error("{0}")]
    Usage(String),
    /// Input file could not be read.
    #[error("failed to read '{path}': {source}")]
    Read {
        /// Display-formatted input path (built at the call site; avoids a
        /// `PathBuf` field, which does not implement `Display`).
        path: String,
        /// Underlying I/O failure.
        source: io::Error,
    },
    /// Bytes were read but no supported container/streams were found.
    #[error("unsupported or unparseable container in '{path}'")]
    Unsupported {
        /// Display-formatted input path.
        path: String,
    },
}

impl ProbeError {
    /// Process exit code: usage errors are `2`, runtime errors are `1`.
    pub(crate) const fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Read { .. } | Self::Unsupported { .. } => 1,
        }
    }
}
