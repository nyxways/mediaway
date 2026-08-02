//! Chunk-stream encoder: basic header, message header types 0-3 (with RTMP's own field-
//! caching-across-chunks rules), extended timestamp, and `chunk_size`-bounded payload
//! fragmentation. Push-append-to-`&mut Vec<u8>`, mirrors `flv::Muxer`'s idiom.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use crate::chunk_common::{EXTENDED_TIMESTAMP_ESCAPE, write_basic_header, write_u24_be};

#[derive(Debug, Clone, Copy)]
struct CachedHeader {
    timestamp_ms: u32,
    message_length: u32,
    message_type_id: u8,
    message_stream_id: u32,
    last_delta: Option<u32>,
}

/// Encodes RTMP messages into the chunk-stream wire format.
///
/// Tracks the standard per-chunk-stream-ID header cache needed to choose the most
/// compressed message-header form (type 0 full / 1 same-stream / 2 same-length-and-type / 3
/// continuation-or-reuse).
#[derive(Debug)]
pub struct ChunkEncoder {
    chunk_size: usize,
    cache: HashMap<u32, CachedHeader>,
}

impl ChunkEncoder {
    /// New encoder with the given max chunk payload size (RTMP's default is 128 before
    /// either side negotiates otherwise via `Set Chunk Size`; `0` is clamped to `1` to avoid
    /// an infinite fragmentation loop).
    #[must_use]
    pub fn new(chunk_size: u32) -> Self {
        let chunk_size = usize::try_from(chunk_size).unwrap_or(usize::MAX).max(1);
        Self {
            chunk_size,
            cache: HashMap::new(),
        }
    }

    /// Update the encoder's own fragmentation size. Call this after appending a
    /// `Set Chunk Size` protocol-control message so subsequent chunks use the new size.
    pub fn set_chunk_size(&mut self, chunk_size: u32) {
        self.chunk_size = usize::try_from(chunk_size).unwrap_or(usize::MAX).max(1);
    }

    /// Encode one RTMP message onto `csid`: a basic header + message header (type chosen via
    /// this encoder's per-`csid` cache) + `chunk_size`-bounded payload fragments, each
    /// continuation fragment prefixed by its own type-3 basic header. Appends to `out`.
    pub fn encode_message(
        &mut self,
        csid: u32,
        message_type_id: u8,
        timestamp_ms: u32,
        message_stream_id: u32,
        payload: &[u8],
        out: &mut Vec<u8>,
    ) {
        let message_length = u32::try_from(payload.len().min(0x00FF_FFFF)).unwrap_or(0x00FF_FFFF); // 24-bit field
        let cached = self.cache.get(&csid).copied();

        let (fmt, delta) = Self::choose_fmt(
            cached,
            message_type_id,
            message_length,
            message_stream_id,
            timestamp_ms,
        );
        let ts_or_delta = if fmt == 0 {
            timestamp_ms
        } else {
            delta.unwrap_or(0)
        };
        let uses_extended = ts_or_delta >= EXTENDED_TIMESTAMP_ESCAPE;

        write_basic_header(out, fmt, csid);
        Self::write_message_header(
            out,
            fmt,
            ts_or_delta,
            message_length,
            message_type_id,
            message_stream_id,
        );
        if uses_extended {
            out.extend_from_slice(&ts_or_delta.to_be_bytes());
        }

        self.cache.insert(
            csid,
            CachedHeader {
                timestamp_ms,
                message_length,
                message_type_id,
                message_stream_id,
                last_delta: if fmt == 0 { None } else { delta },
            },
        );

        let mut remaining = payload;
        let first_len = remaining.len().min(self.chunk_size);
        out.extend_from_slice(&remaining[..first_len]);
        remaining = &remaining[first_len..];

        while !remaining.is_empty() {
            write_basic_header(out, 3, csid);
            if uses_extended {
                out.extend_from_slice(&ts_or_delta.to_be_bytes());
            }
            let n = remaining.len().min(self.chunk_size);
            out.extend_from_slice(&remaining[..n]);
            remaining = &remaining[n..];
        }
    }

    fn write_message_header(
        out: &mut Vec<u8>,
        fmt: u8,
        ts_or_delta: u32,
        message_length: u32,
        message_type_id: u8,
        message_stream_id: u32,
    ) {
        let ts_field = ts_or_delta.min(EXTENDED_TIMESTAMP_ESCAPE);
        match fmt {
            0 => {
                write_u24_be(out, ts_field);
                write_u24_be(out, message_length);
                out.push(message_type_id);
                out.extend_from_slice(&message_stream_id.to_le_bytes());
            }
            1 => {
                write_u24_be(out, ts_field);
                write_u24_be(out, message_length);
                out.push(message_type_id);
            }
            2 => {
                write_u24_be(out, ts_field);
            }
            _ => {}
        }
    }

    /// Choose the most compressed message-header type for the next message on a chunk
    /// stream, given that stream's cached previous header (if any). Returns the delta to
    /// write to the wire for fmt 1/2/3 (not meaningful for fmt 0, which carries an absolute
    /// timestamp).
    fn choose_fmt(
        cached: Option<CachedHeader>,
        message_type_id: u8,
        message_length: u32,
        message_stream_id: u32,
        timestamp_ms: u32,
    ) -> (u8, Option<u32>) {
        let Some(c) = cached else {
            return (0, None);
        };
        if c.message_stream_id != message_stream_id {
            return (0, None);
        }
        let delta = timestamp_ms.wrapping_sub(c.timestamp_ms);
        if c.message_type_id != message_type_id || c.message_length != message_length {
            return (1, Some(delta));
        }
        if c.last_delta != Some(delta) {
            return (2, Some(delta));
        }
        (3, Some(delta))
    }
}

#[cfg(test)]
#[path = "chunk_encoder_tests.rs"]
mod tests;
