//! FLV file-header + tag writer.

#![forbid(unsafe_code)]

use crate::error::Error;
use crate::types::Tag;

const TAG_HEADER_LEN: usize = 11;
const MAX_DATA_SIZE: usize = 0x00FF_FFFF; // 24-bit DataSize field

/// Writes an FLV file header followed by tags, each self-trailed with its own
/// `PreviousTagSize` (FLV has no incremental flush concept beyond "append the
/// next complete tag" — there is no `finish()` step).
#[derive(Debug, Clone, Copy, Default)]
pub struct Muxer {
    header_written: bool,
}

impl Muxer {
    /// New mux session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Write the 9-byte FLV file header + the leading 4-byte `PreviousTagSize0` (always 0).
    pub fn write_header(&mut self, has_audio: bool, has_video: bool, out: &mut Vec<u8>) {
        out.extend_from_slice(b"FLV");
        out.push(1); // version
        let flags = (u8::from(has_audio) << 2) | u8::from(has_video);
        out.push(flags);
        out.extend_from_slice(&9u32.to_be_bytes()); // DataOffset: standard 9-byte header
        out.extend_from_slice(&0u32.to_be_bytes()); // PreviousTagSize0
        self.header_written = true;
    }

    /// Append one tag (11-byte header + data + trailing `PreviousTagSize`) to `out`.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "data_size is bounds-checked against MAX_DATA_SIZE just above; ts>>24 always fits u8"
    )]
    pub fn write_tag(&self, tag: &Tag, out: &mut Vec<u8>) -> Result<(), Error> {
        if !self.header_written {
            return Err(Error::HeaderNotWritten);
        }
        if tag.data.len() > MAX_DATA_SIZE {
            return Err(Error::TagDataTooLarge(tag.data.len()));
        }
        let data_size = tag.data.len();
        let data_size_bytes = (data_size as u32).to_be_bytes(); // top byte always 0 (checked above)

        out.push(tag.tag_type.value());
        out.extend_from_slice(&data_size_bytes[1..4]);
        let ts = tag.timestamp_ms;
        out.extend_from_slice(&ts.to_be_bytes()[1..4]); // lower 24 bits, big-endian
        out.push((ts >> 24) as u8); // TimestampExtended: upper 8 bits
        out.extend_from_slice(&[0, 0, 0]); // StreamID: always 0
        out.extend_from_slice(&tag.data);

        let tag_len = u32::try_from(TAG_HEADER_LEN + data_size).unwrap_or(u32::MAX);
        out.extend_from_slice(&tag_len.to_be_bytes());
        Ok(())
    }
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod tests;
