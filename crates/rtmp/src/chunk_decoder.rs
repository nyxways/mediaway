//! Chunk-stream decoder: incremental `push_bytes`/`poll_message`, reassembling `chunk_size`-
//! bounded fragments (default 128, or whatever the peer's own `Set Chunk Size` message last
//! announced — recognized and applied internally) back into `(message_type_id, timestamp_ms,
//! payload)` tuples. Mirrors `flv::Demuxer`'s incremental `push_bytes`/`poll_*` style.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use crate::chunk_common::{
    self as common, BasicHeader, DEFAULT_CHUNK_SIZE, EXTENDED_TIMESTAMP_ESCAPE, MSG_SET_CHUNK_SIZE,
    read_basic_header, read_u24_be,
};
use crate::error::Error;

#[derive(Debug, Clone, Copy)]
struct HeaderCache {
    timestamp_ms: u32,
    message_length: u32,
    message_type_id: u8,
    message_stream_id: u32,
    last_delta: Option<u32>,
    uses_extended: bool,
}

#[derive(Debug, Default)]
struct CsidState {
    cache: Option<HeaderCache>,
    /// Bytes of the message currently being reassembled on this chunk stream, if any. Reset
    /// to empty once a message completes, so the next type-3 chunk on this `csid` is
    /// correctly read as "new message reusing the cache" rather than a continuation.
    received: Vec<u8>,
}

enum Step {
    NeedMoreBytes,
    Continue,
    Message(u8, u32, Vec<u8>),
}

/// Reads chunk-stream bytes and reassembles complete RTMP messages.
#[derive(Debug)]
pub struct ChunkDecoder {
    buf: Vec<u8>,
    chunk_size: usize,
    state: HashMap<u32, CsidState>,
}

impl ChunkDecoder {
    /// New decoder assuming the RTMP-default 128-byte chunk size until a `Set Chunk Size`
    /// message is recognized in the incoming stream.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            chunk_size: DEFAULT_CHUNK_SIZE,
            state: HashMap::new(),
        }
    }

    /// Append incoming bytes.
    pub fn push_bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Override the decoder's assumed peer chunk size directly, without waiting to observe
    /// a `Set Chunk Size` message on the wire. Normally unnecessary when decoding a real
    /// stream produced by this crate's own [`crate::Muxer`] (which always sends that
    /// message first) — useful when composing [`crate::ChunkEncoder`]/`ChunkDecoder`
    /// directly at a non-default chunk size without also encoding that control message.
    pub fn set_chunk_size(&mut self, chunk_size: u32) {
        self.chunk_size = usize::try_from(chunk_size).unwrap_or(usize::MAX).max(1);
    }

    /// Pop the next complete message, or `Ok(None)` if not enough bytes are buffered yet —
    /// call again after more `push_bytes`. Internally consumes as many complete chunks as
    /// are buffered (possibly across several chunk-stream IDs) until one message completes.
    pub fn poll_message(&mut self) -> Result<Option<(u8, u32, Vec<u8>)>, Error> {
        loop {
            match self.step()? {
                Step::NeedMoreBytes => return Ok(None),
                Step::Continue => {}
                Step::Message(message_type_id, timestamp_ms, payload) => {
                    return Ok(Some((message_type_id, timestamp_ms, payload)));
                }
            }
        }
    }

    fn step(&mut self) -> Result<Step, Error> {
        let Some(basic) = read_basic_header(&self.buf) else {
            return Ok(Step::NeedMoreBytes);
        };
        let is_continuation = basic.fmt == 3
            && self
                .state
                .get(&basic.csid)
                .is_some_and(|s| !s.received.is_empty());

        if is_continuation {
            self.step_continuation(&basic)
        } else {
            self.step_new_message(&basic)
        }
    }

    fn step_continuation(&mut self, basic: &BasicHeader) -> Result<Step, Error> {
        let Some(state) = self.state.get(&basic.csid) else {
            return Ok(Step::NeedMoreBytes);
        };
        let Some(cache) = state.cache else {
            return Ok(Step::NeedMoreBytes);
        };
        let ext_len = usize::from(cache.uses_extended) * 4;
        let header_total = basic.len + ext_len;
        let expected = usize::try_from(cache.message_length).unwrap_or(usize::MAX);
        let remaining = expected.saturating_sub(state.received.len());
        let fragment_len = remaining.min(self.chunk_size);

        if self.buf.len() < header_total + fragment_len {
            return Ok(Step::NeedMoreBytes);
        }

        let fragment = self.buf[header_total..header_total + fragment_len].to_vec();
        self.buf.drain(0..header_total + fragment_len);

        if let Some(state) = self.state.get_mut(&basic.csid) {
            state.received.extend_from_slice(&fragment);
        }

        self.finish_if_complete(basic.csid)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one cohesive fmt 0-3 header resolution table; splitting would scatter it"
    )]
    fn step_new_message(&mut self, basic: &BasicHeader) -> Result<Step, Error> {
        let msg_header_len: usize = match basic.fmt {
            0 => 11,
            1 => 7,
            2 => 3,
            _ => 0,
        };
        if self.buf.len() < basic.len + msg_header_len {
            return Ok(Step::NeedMoreBytes);
        }
        let cached = self.state.get(&basic.csid).and_then(|s| s.cache);

        let hdr = &self.buf[basic.len..basic.len + msg_header_len];
        let (ts_field, message_length, message_type_id, message_stream_id) = match basic.fmt {
            0 => (
                read_u24_be(&hdr[0..3]),
                read_u24_be(&hdr[3..6]),
                hdr[6],
                common::read_u32_le(&hdr[7..11]),
            ),
            1 => {
                let c = cached.ok_or(Error::NoCachedHeader(basic.csid))?;
                (
                    read_u24_be(&hdr[0..3]),
                    read_u24_be(&hdr[3..6]),
                    hdr[6],
                    c.message_stream_id,
                )
            }
            2 => {
                let c = cached.ok_or(Error::NoCachedHeader(basic.csid))?;
                (
                    read_u24_be(&hdr[0..3]),
                    c.message_length,
                    c.message_type_id,
                    c.message_stream_id,
                )
            }
            _ => {
                let c = cached.ok_or(Error::NoCachedHeader(basic.csid))?;
                (0, c.message_length, c.message_type_id, c.message_stream_id)
            }
        };

        let uses_extended = match basic.fmt {
            0..=2 => ts_field == EXTENDED_TIMESTAMP_ESCAPE,
            _ => cached.is_some_and(|c| c.uses_extended),
        };
        let ext_len = if uses_extended { 4 } else { 0 };
        let total_header = basic.len + msg_header_len + ext_len;

        let payload_cap = usize::try_from(message_length).unwrap_or(usize::MAX);
        let first_fragment_len = payload_cap.min(self.chunk_size);
        if self.buf.len() < total_header + first_fragment_len {
            return Ok(Step::NeedMoreBytes);
        }

        let ext_value = if uses_extended {
            Some(common::read_u32_be(
                &self.buf[basic.len + msg_header_len..basic.len + msg_header_len + 4],
            ))
        } else {
            None
        };

        let (timestamp_ms, delta_for_cache) = match basic.fmt {
            0 => (ext_value.unwrap_or(ts_field), None),
            1 | 2 => {
                let c = cached.ok_or(Error::NoCachedHeader(basic.csid))?;
                let delta = ext_value.unwrap_or(ts_field);
                (c.timestamp_ms.wrapping_add(delta), Some(delta))
            }
            _ => {
                let c = cached.ok_or(Error::NoCachedHeader(basic.csid))?;
                let delta = c.last_delta.unwrap_or(0);
                (c.timestamp_ms.wrapping_add(delta), Some(delta))
            }
        };

        let fragment = self.buf[total_header..total_header + first_fragment_len].to_vec();
        self.buf.drain(0..total_header + first_fragment_len);

        let entry = self.state.entry(basic.csid).or_default();
        entry.cache = Some(HeaderCache {
            timestamp_ms,
            message_length,
            message_type_id,
            message_stream_id,
            last_delta: delta_for_cache,
            uses_extended,
        });
        entry.received = fragment;

        self.finish_if_complete(basic.csid)
    }

    fn finish_if_complete(&mut self, csid: u32) -> Result<Step, Error> {
        let Some(state) = self.state.get_mut(&csid) else {
            return Ok(Step::Continue);
        };
        let Some(cache) = state.cache else {
            return Ok(Step::Continue);
        };
        let expected = usize::try_from(cache.message_length).unwrap_or(usize::MAX);
        if state.received.len() < expected {
            return Ok(Step::Continue);
        }
        let payload = std::mem::take(&mut state.received);
        let message_type_id = cache.message_type_id;
        let timestamp_ms = cache.timestamp_ms;

        if message_type_id == MSG_SET_CHUNK_SIZE
            && let [b0, b1, b2, b3] = payload[..]
        {
            let value = u32::from_be_bytes([b0 & 0x7F, b1, b2, b3]); // top bit reserved
            if value == 0 {
                return Err(Error::InvalidChunkSize(value));
            }
            self.chunk_size = usize::try_from(value).unwrap_or(usize::MAX);
        }

        Ok(Step::Message(message_type_id, timestamp_ms, payload))
    }
}

impl Default for ChunkDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "chunk_decoder_tests.rs"]
mod tests;
