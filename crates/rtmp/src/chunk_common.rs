//! Shared chunk-stream basic-header encode/decode + message-type-id constants used by both
//! [`crate::ChunkEncoder`] and [`crate::ChunkDecoder`]. Crate-internal — not part of this
//! crate's public API (the encoder/decoder types built on top of it are).

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "explicit pub(crate) matches this workspace's unreachable_pub convention; clippy's \
              suggested plain `pub` would make these items look crate-public by accident"
)]

/// Default RTMP chunk payload size before either side sends `Set Chunk Size`.
pub(crate) const DEFAULT_CHUNK_SIZE: usize = 128;

/// Protocol-control message: `Set Chunk Size` (4-byte big-endian payload).
pub(crate) const MSG_SET_CHUNK_SIZE: u8 = 1;
/// Audio message.
pub(crate) const MSG_AUDIO: u8 = 8;
/// Video message.
pub(crate) const MSG_VIDEO: u8 = 9;
/// AMF0 data message (e.g. `onMetaData`).
pub(crate) const MSG_AMF0_DATA: u8 = 18;
/// AMF0 command message (e.g. `connect`, `createStream`, `publish`).
pub(crate) const MSG_AMF0_COMMAND: u8 = 20;

/// Timestamp/delta escape value: when the 3-byte header field reads `0xFF_FFFF`, a 4-byte
/// big-endian extended value follows the message header (and is repeated on every
/// subsequent type-3 chunk of the same message/header reuse).
pub(crate) const EXTENDED_TIMESTAMP_ESCAPE: u32 = 0x00FF_FFFF;

/// A parsed chunk basic header: `fmt` (0-3), `csid`, and how many bytes it occupied on the
/// wire.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BasicHeader {
    pub(crate) fmt: u8,
    pub(crate) csid: u32,
    pub(crate) len: usize,
}

/// Append a chunk basic header: the top 2 bits of the first byte are `fmt`; the rest encode
/// `csid` in RTMP's 1/2/3-byte form (`csid < 64` → 1 byte; `64..320` → 2 bytes; `320..=65599`
/// → 3 bytes, little-endian extension per spec).
#[allow(
    clippy::cast_possible_truncation,
    reason = "each branch's csid arithmetic is bounds-checked by the preceding if/else before the cast"
)]
pub(crate) fn write_basic_header(out: &mut Vec<u8>, fmt: u8, csid: u32) {
    debug_assert!(fmt <= 3, "fmt must be 0..=3");
    if csid < 64 {
        out.push((fmt << 6) | csid as u8);
    } else if csid < 320 {
        out.push(fmt << 6);
        out.push((csid - 64) as u8);
    } else {
        out.push((fmt << 6) | 0x01);
        let v = csid - 64;
        out.push((v & 0xFF) as u8);
        out.push((v >> 8) as u8);
    }
}

/// Try to parse a basic header from the start of `buf`. Returns `None` if `buf` doesn't yet
/// hold enough bytes (caller should wait for more via `push_bytes`).
pub(crate) fn read_basic_header(buf: &[u8]) -> Option<BasicHeader> {
    let b0 = *buf.first()?;
    let fmt = b0 >> 6;
    let low6 = b0 & 0x3F;
    match low6 {
        0 => {
            let b1 = *buf.get(1)?;
            Some(BasicHeader {
                fmt,
                csid: 64 + u32::from(b1),
                len: 2,
            })
        }
        1 => {
            let b1 = *buf.get(1)?;
            let b2 = *buf.get(2)?;
            Some(BasicHeader {
                fmt,
                csid: 64 + u32::from(b1) + u32::from(b2) * 256,
                len: 3,
            })
        }
        csid => Some(BasicHeader {
            fmt,
            csid: u32::from(csid),
            len: 1,
        }),
    }
}

/// Append a 24-bit big-endian value (the top 8 bits of `value` are dropped — callers
/// pre-clamp to `0x00FF_FFFF`).
pub(crate) fn write_u24_be(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes()[1..]);
}

/// Read a 24-bit big-endian value from `buf[0..3]`. Callers must length-check first (mirrors
/// `flv::Demuxer`'s direct-indexing style after a bounds check).
pub(crate) fn read_u24_be(buf: &[u8]) -> u32 {
    (u32::from(buf[0]) << 16) | (u32::from(buf[1]) << 8) | u32::from(buf[2])
}

/// Read a 32-bit big-endian value from `buf[0..4]`.
pub(crate) fn read_u32_be(buf: &[u8]) -> u32 {
    u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

/// Read a 32-bit **little-endian** value from `buf[0..4]` — RTMP's `MessageStreamID` field
/// in a type-0 chunk message header is little-endian, an established spec quirk (unlike
/// every other multi-byte field in the chunk header, which is big-endian).
pub(crate) fn read_u32_le(buf: &[u8]) -> u32 {
    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

#[cfg(test)]
#[path = "chunk_common_tests.rs"]
mod tests;
