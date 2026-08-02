//! Chunk-stream message demux — `push_bytes`/`poll_message`, no AMF0 decode (see
//! `adr/0001-rtmp-freestanding-core.md` § 3). Composes [`ChunkDecoder`].

#![forbid(unsafe_code)]

use crate::chunk_decoder::ChunkDecoder;
use crate::error::Error;

/// Reads chunk-stream bytes into raw `(message_type_id, timestamp_ms, payload)` tuples —
/// message-boundary only, no AMF0 command/data interpretation.
#[derive(Debug, Default)]
pub struct Demuxer {
    decoder: ChunkDecoder,
}

impl Demuxer {
    /// New, empty demux session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append incoming bytes.
    pub fn push_bytes(&mut self, chunk: &[u8]) {
        self.decoder.push_bytes(chunk);
    }

    /// Pop the next complete message, or `Ok(None)` if not enough bytes are buffered yet
    /// (call again after more `push_bytes`).
    ///
    /// Diverges from `adr/0001-rtmp-freestanding-core.md` § 4's literal `Option<...>`
    /// signature: chunk-stream decode can genuinely fail on malformed input (a type-3 chunk
    /// with no prior cached header, an invalid `Set Chunk Size` value), unlike AMF0 encode —
    /// this matches `flv::Demuxer::poll_tag`'s own `Result<Option<T>, Error>` idiom rather
    /// than silently swallowing a real parse error.
    pub fn poll_message(&mut self) -> Result<Option<(u8, u32, Vec<u8>)>, Error> {
        self.decoder.poll_message()
    }
}

#[cfg(test)]
#[path = "demux_tests.rs"]
mod tests;
