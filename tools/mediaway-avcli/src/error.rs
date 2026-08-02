//! CLI error type — maps to an exit code and an English stderr message.

use mediaway_container::mp4::Error as MuxError;
use thiserror::Error;

/// Top-level avcli error.
#[derive(Debug, Error)]
pub(crate) enum CliError {
    /// Bad or missing command-line arguments.
    #[error("{0}")]
    Usage(String),
    /// I/O failure reading input or writing output.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Container mux failure.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),
}

impl CliError {
    /// Process exit code: usage errors are `2`, runtime errors are `1`.
    pub(crate) const fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Io(_) | Self::Mux(_) => 1,
        }
    }
}
