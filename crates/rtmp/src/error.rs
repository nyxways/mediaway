//! Public error type.

#![forbid(unsafe_code)]

/// Errors from RTMP handshake, chunk-stream, and AMF0 encode.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An AMF0 string or object/array property name is longer than 65,535 bytes — AMF0's
    /// 16-bit length prefix cannot represent it.
    #[error("AMF0 string of {0} bytes exceeds the 16-bit length prefix (max 65,535)")]
    StringTooLong(usize),
    /// `S0`'s version byte was not `0x03` (plain RTMP; RTMPE's `0x06` is out of scope, see
    /// `adr/0001-rtmp-freestanding-core.md` § Non-Goals).
    #[error("unexpected S0 version byte {0:#04x} (expected 0x03)")]
    UnexpectedS0Version(u8),
    /// Neither digest-offset layout's HMAC-SHA256 validated against the received `S1` — the
    /// server's digest could not be located, so `Handshake` cannot derive `C2`.
    #[error("could not locate S1's digest (neither offset layout validated)")]
    S1DigestNotFound,
    /// An HMAC-SHA256 key was rejected by the `hmac` crate. Not expected to occur: this
    /// crate only ever uses its own fixed-length key constants/derived 32-byte temp keys.
    /// Surfaced instead of `unwrap`/`expect` per workspace policy.
    #[error("HMAC key rejected (unexpected — this crate only uses fixed-length key constants)")]
    HmacKeyLength,
    /// A `Set Chunk Size` protocol-control message carried an invalid value. RTMP chunk
    /// sizes are a positive value in a 31-bit field (top bit reserved).
    #[error("invalid chunk size {0} from peer (must be nonzero)")]
    InvalidChunkSize(u32),
    /// A type-3 chunk arrived for a chunk-stream ID with no prior cached header — the peer
    /// sent a continuation/reuse chunk before ever sending a type-0 header for this ID.
    #[error("type-3 chunk for chunk stream {0} with no prior cached header")]
    NoCachedHeader(u32),
}
