//! ADTS frame reader — incremental, byte-chunk push/poll.

#![forbid(unsafe_code)]

use bytes::Bytes;

use crate::error::Error;
use crate::types::{AacProfile, AdtsConfig, sample_rate_from_index};

const HEADER_LEN_NO_CRC: usize = 7;
const HEADER_LEN_WITH_CRC: usize = 9;

/// Reads back-to-back ADTS frames from pushed byte chunks.
#[derive(Debug, Clone, Default)]
pub struct Demuxer {
    buf: Vec<u8>,
    config: Option<AdtsConfig>,
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

    /// `AdtsConfig` parsed from the most recently returned frame's header, if any.
    #[must_use]
    pub const fn config(&self) -> Option<AdtsConfig> {
        self.config
    }

    /// Pop the next complete frame's raw AAC payload (ADTS header stripped), or
    /// `Ok(None)` if the buffer doesn't yet hold a full frame — call again after
    /// more `push_bytes`. Errors on a bad sync word or a reserved
    /// `sampling_frequency_index`, rather than silently emitting garbage.
    pub fn poll_frame(&mut self) -> Result<Option<Bytes>, Error> {
        if self.buf.len() < HEADER_LEN_NO_CRC {
            return Ok(None);
        }
        if self.buf[0] != 0xFF || (self.buf[1] & 0xF0) != 0xF0 {
            return Err(Error::BadSync);
        }
        let protection_absent = (self.buf[1] & 0x01) != 0;
        let header_len = if protection_absent {
            HEADER_LEN_NO_CRC
        } else {
            HEADER_LEN_WITH_CRC
        };
        if self.buf.len() < header_len {
            return Ok(None);
        }

        let profile_bits = (self.buf[2] >> 6) & 0x03;
        let sfi = (self.buf[2] >> 2) & 0x0F;
        let channels = ((self.buf[2] & 0x01) << 2) | ((self.buf[3] >> 6) & 0x03);
        let frame_len = ((usize::from(self.buf[3]) & 0x03) << 11)
            | (usize::from(self.buf[4]) << 3)
            | (usize::from(self.buf[5]) >> 5);

        if self.buf.len() < frame_len {
            return Ok(None);
        }
        if frame_len < header_len {
            return Err(Error::BadSync);
        }

        let sample_rate =
            sample_rate_from_index(sfi).ok_or(Error::UnsupportedSamplingFrequencyIndex(sfi))?;
        self.config = Some(AdtsConfig {
            profile: AacProfile::from_bits(profile_bits),
            sample_rate,
            channels,
        });

        let payload = Bytes::copy_from_slice(&self.buf[header_len..frame_len]);
        self.buf.drain(0..frame_len);
        Ok(Some(payload))
    }
}

#[cfg(test)]
#[path = "demux_tests.rs"]
mod tests;
