//! MPEG audio (Layer III) frame reader — incremental, byte-chunk push/poll.

#![forbid(unsafe_code)]

use bytes::Bytes;

use crate::error::Error;
use crate::types::{ChannelMode, FrameHeader, MpegVersion, bitrate_table, sample_rate_table};

const HEADER_LEN: usize = 4;
const CRC_LEN: usize = 2;

const fn version_from_bits(bits: u8) -> Option<MpegVersion> {
    match bits & 0x03 {
        0b00 => Some(MpegVersion::Mpeg25),
        0b10 => Some(MpegVersion::Mpeg2),
        0b11 => Some(MpegVersion::Mpeg1),
        _ => None, // 0b01 reserved
    }
}

/// Reads back-to-back MPEG audio (Layer III) frames from pushed byte chunks.
///
/// Assumes the input is already frame-aligned (no ID3 tag or leading garbage
/// skipping) — a bad sync word or reserved header field is a hard `Err`, never a
/// silent resync scan (matches this workspace's `adts-core` crate).
#[derive(Debug, Clone, Default)]
pub struct Demuxer {
    buf: Vec<u8>,
    header: Option<FrameHeader>,
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

    /// `FrameHeader` parsed from the most recently returned frame, if any.
    #[must_use]
    pub const fn header(&self) -> Option<FrameHeader> {
        self.header
    }

    /// Pop the next complete frame's payload (header + optional CRC stripped), or
    /// `Ok(None)` if the buffer doesn't yet hold a full frame.
    pub fn poll_frame(&mut self) -> Result<Option<Bytes>, Error> {
        if self.buf.len() < HEADER_LEN {
            return Ok(None);
        }
        if self.buf[0] != 0xFF || (self.buf[1] & 0xE0) != 0xE0 {
            return Err(Error::BadSyncOrReservedField);
        }
        let version = version_from_bits(self.buf[1] >> 3).ok_or(Error::BadSyncOrReservedField)?;
        let layer_bits = (self.buf[1] >> 1) & 0x03;
        if layer_bits != 0b01 {
            return Err(Error::UnsupportedLayer);
        }
        let protection_absent = (self.buf[1] & 0x01) != 0;

        let bitrate_index = (self.buf[2] >> 4) & 0x0F;
        if bitrate_index == 0 || bitrate_index == 0x0F {
            return Err(Error::BadSyncOrReservedField);
        }
        let bitrate_kbps = bitrate_table(version)[usize::from(bitrate_index) - 1];

        let sample_rate_index = (self.buf[2] >> 2) & 0x03;
        if sample_rate_index == 0x03 {
            return Err(Error::BadSyncOrReservedField);
        }
        let sample_rate = sample_rate_table(version)[usize::from(sample_rate_index)];

        let padding = (self.buf[2] >> 1) & 0x01 != 0;
        let channel_mode = ChannelMode::from_bits(self.buf[3] >> 6);

        let header = FrameHeader {
            version,
            bitrate_kbps,
            sample_rate,
            channel_mode,
        };
        let frame_len = header.frame_len(padding);
        if self.buf.len() < frame_len {
            return Ok(None);
        }

        let payload_start = if protection_absent {
            HEADER_LEN
        } else {
            HEADER_LEN + CRC_LEN
        };
        self.header = Some(header);
        let payload = Bytes::copy_from_slice(&self.buf[payload_start.min(frame_len)..frame_len]);
        self.buf.drain(0..frame_len);
        Ok(Some(payload))
    }
}

#[cfg(test)]
#[path = "demux_tests.rs"]
mod tests;
