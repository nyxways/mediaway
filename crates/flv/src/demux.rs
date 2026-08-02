//! FLV file-header + tag reader — incremental, byte-chunk push/poll.

#![forbid(unsafe_code)]

use bytes::Bytes;

use crate::error::Error;
use crate::types::{Tag, TagType};

const TAG_HEADER_LEN: usize = 11;
const TRAILER_LEN: usize = 4; // PreviousTagSize

/// Reads an FLV file header followed by tags from pushed byte chunks.
#[derive(Debug, Clone, Default)]
pub struct Demuxer {
    buf: Vec<u8>,
    header_parsed: bool,
    has_audio: bool,
    has_video: bool,
}

impl Demuxer {
    /// New, empty demux session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append incoming bytes.
    pub fn push_bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Whether the file header (parsed) declares audio tags present.
    #[must_use]
    pub const fn has_audio(&self) -> Option<bool> {
        if self.header_parsed {
            Some(self.has_audio)
        } else {
            None
        }
    }

    /// Whether the file header (parsed) declares video tags present.
    #[must_use]
    pub const fn has_video(&self) -> Option<bool> {
        if self.header_parsed {
            Some(self.has_video)
        } else {
            None
        }
    }

    /// Pop the next complete tag, or `Ok(None)` if not enough bytes are buffered
    /// yet — call again after more `push_bytes`.
    pub fn poll_tag(&mut self) -> Result<Option<Tag>, Error> {
        if !self.header_parsed && !self.parse_header()? {
            return Ok(None);
        }
        self.parse_tag()
    }

    fn parse_header(&mut self) -> Result<bool, Error> {
        if self.buf.len() < 9 {
            return Ok(false);
        }
        if &self.buf[0..3] != b"FLV" {
            return Err(Error::BadSignature);
        }
        let flags = self.buf[4];
        let data_offset =
            u32::from_be_bytes(self.buf[5..9].try_into().unwrap_or_default()) as usize;
        let total_header = data_offset + TRAILER_LEN;
        if self.buf.len() < total_header {
            return Ok(false);
        }
        self.has_video = flags & 0x01 != 0;
        self.has_audio = flags & 0x04 != 0;
        self.header_parsed = true;
        self.buf.drain(0..total_header);
        Ok(true)
    }

    fn parse_tag(&mut self) -> Result<Option<Tag>, Error> {
        if self.buf.len() < TAG_HEADER_LEN {
            return Ok(None);
        }
        let tag_type =
            TagType::from_value(self.buf[0]).ok_or(Error::UnknownTagType(self.buf[0]))?;
        let data_size = (usize::from(self.buf[1]) << 16)
            | (usize::from(self.buf[2]) << 8)
            | usize::from(self.buf[3]);
        let ts_low =
            u32::from(self.buf[4]) << 16 | u32::from(self.buf[5]) << 8 | u32::from(self.buf[6]);
        let ts_ext = u32::from(self.buf[7]);
        let timestamp_ms = (ts_ext << 24) | ts_low;

        let total = TAG_HEADER_LEN + data_size + TRAILER_LEN;
        if self.buf.len() < total {
            return Ok(None);
        }
        let data = Bytes::copy_from_slice(&self.buf[TAG_HEADER_LEN..TAG_HEADER_LEN + data_size]);
        self.buf.drain(0..total);
        Ok(Some(Tag {
            tag_type,
            timestamp_ms,
            data,
        }))
    }
}

#[cfg(test)]
#[path = "demux_tests.rs"]
mod tests;
