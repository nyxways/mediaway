//! Public error type.

#![forbid(unsafe_code)]

/// Errors from Ogg mux/demux.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// First 4 bytes are not `OggS`.
    #[error("bad Ogg capture pattern (expected \"OggS\")")]
    BadCapturePattern,
    /// Page `version` byte is not `0`.
    #[error("unsupported Ogg page version {0} (only 0 is defined)")]
    UnsupportedVersion(u8),
    /// Page CRC-32 doesn't match the computed value over the page bytes.
    #[error("Ogg page CRC mismatch: header claims {expected:#010x}, computed {computed:#010x}")]
    CrcMismatch {
        /// CRC from the page header.
        expected: u32,
        /// CRC recomputed over the page bytes.
        computed: u32,
    },
    /// A page's `continued packet` flag doesn't match whether a partial packet is
    /// actually buffered — a real stream desync, not a value worth guessing past.
    #[error("Ogg page continuation flag ({flag}) doesn't match buffered partial-packet state")]
    ContinuationFlagMismatch {
        /// The page's `continued packet` header bit.
        flag: bool,
    },
    /// `Muxer::push_packet`'s packet doesn't fit in a single page's 255-entry
    /// segment table (max 65024 bytes) — multi-page packet splitting is a v1
    /// scope gap (crate-local ADR-0001).
    #[error(
        "packet of {0} bytes doesn't fit in a single Ogg page (max 65024 bytes; multi-page splitting not yet implemented)"
    )]
    PacketTooLargeForSinglePage(usize),
}
