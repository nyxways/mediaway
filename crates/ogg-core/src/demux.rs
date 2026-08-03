//! Ogg page reader — incremental, byte-chunk push/poll. Reassembles packets
//! that span continuation pages and pages carrying multiple packets (the
//! general case any real Ogg encoder produces, even though this crate's own
//! [`crate::Muxer`] only ever emits the simpler one-packet-per-page form).

#![forbid(unsafe_code)]

use std::collections::VecDeque;

use bytes::Bytes;

use crate::crc::crc32_ogg;
use crate::error::Error;
use crate::types::Packet;

const HEADER_LEN: usize = 27; // capture(4) + version(1) + flags(1) + granule(8) + serial(4) + sequence(4) + crc(4) + page_segments(1)
const CRC_FIELD_OFFSET: usize = 4 + 1 + 1 + 8 + 4 + 4;

/// Reads Ogg pages from pushed byte chunks and yields fully reassembled packets.
#[derive(Debug, Default)]
pub struct Demuxer {
    buf: Vec<u8>,
    partial: Vec<u8>,
    has_partial: bool,
    ready: VecDeque<Packet>,
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

    /// Pop the next fully reassembled packet, or `Ok(None)` if no more pages are
    /// buffered yet — call again after more `push_bytes`.
    pub fn poll_packet(&mut self) -> Result<Option<Packet>, Error> {
        loop {
            if let Some(packet) = self.ready.pop_front() {
                return Ok(Some(packet));
            }
            if !self.parse_one_page()? {
                return Ok(None);
            }
        }
    }

    /// Try to parse and consume one page from `self.buf`. Returns `Ok(true)` if a
    /// page was parsed (queuing 0+ packets into `self.ready`), `Ok(false)` if not
    /// enough bytes are buffered yet.
    fn parse_one_page(&mut self) -> Result<bool, Error> {
        if self.buf.len() < HEADER_LEN {
            return Ok(false);
        }
        if &self.buf[0..4] != b"OggS" {
            return Err(Error::BadCapturePattern);
        }
        let version = self.buf[4];
        if version != 0 {
            return Err(Error::UnsupportedVersion(version));
        }
        let flags = self.buf[5];
        let continued = flags & 0x01 != 0;
        let bos = flags & 0x02 != 0;
        let eos = flags & 0x04 != 0;
        let granule_position = i64::from_le_bytes(self.buf[6..14].try_into().unwrap_or_default());
        let serial = u32::from_le_bytes(self.buf[14..18].try_into().unwrap_or_default());
        let crc_declared = u32::from_le_bytes(self.buf[22..26].try_into().unwrap_or_default());
        let page_segments = usize::from(self.buf[26]);
        let with_seg_table_len = HEADER_LEN + page_segments;
        if self.buf.len() < with_seg_table_len {
            return Ok(false);
        }
        let segment_table = self.buf[HEADER_LEN..with_seg_table_len].to_vec();
        let payload_len: usize = segment_table.iter().map(|&s| usize::from(s)).sum();
        let total_page_len = with_seg_table_len + payload_len;
        if self.buf.len() < total_page_len {
            return Ok(false);
        }

        if continued != self.has_partial {
            return Err(Error::ContinuationFlagMismatch { flag: continued });
        }

        let mut page_for_crc = self.buf[0..total_page_len].to_vec();
        page_for_crc[CRC_FIELD_OFFSET..CRC_FIELD_OFFSET + 4].fill(0);
        let computed = crc32_ogg(&page_for_crc);
        if computed != crc_declared {
            return Err(Error::CrcMismatch {
                expected: crc_declared,
                computed,
            });
        }

        let payload_start = with_seg_table_len;
        let mut seg_start = payload_start;
        let mut offset = payload_start;
        for &seg in &segment_table {
            offset += usize::from(seg);
            if seg < 255 {
                let chunk = &self.buf[seg_start..offset];
                seg_start = offset;
                if self.has_partial {
                    self.partial.extend_from_slice(chunk);
                    self.ready.push_back(Packet {
                        data: Bytes::copy_from_slice(&self.partial),
                        granule_position,
                        serial,
                        bos,
                        eos,
                    });
                    self.partial.clear();
                    self.has_partial = false;
                } else {
                    self.ready.push_back(Packet {
                        data: Bytes::copy_from_slice(chunk),
                        granule_position,
                        serial,
                        bos,
                        eos,
                    });
                }
            }
        }
        if seg_start < total_page_len {
            self.partial
                .extend_from_slice(&self.buf[seg_start..total_page_len]);
            self.has_partial = true;
        }

        self.buf.drain(0..total_page_len);
        Ok(true)
    }
}

#[cfg(test)]
#[path = "demux_tests.rs"]
mod tests;
