//! MPEG audio (Layer III) frame writer — one 4-byte header per already-encoded frame body.

#![forbid(unsafe_code)]

use crate::error::Error;
use crate::types::{FrameHeader, MpegVersion, bitrate_table, sample_rate_table};

const HEADER_LEN: usize = 4;
const LAYER_III_BITS: u8 = 0b01;

const fn version_bits(version: MpegVersion) -> u8 {
    match version {
        MpegVersion::Mpeg25 => 0b00,
        MpegVersion::Mpeg2 => 0b10,
        MpegVersion::Mpeg1 => 0b11,
    }
}

/// Writes MPEG-1/2/2.5 Layer III frame headers for a fixed [`FrameHeader`].
///
/// This crate frames already-encoded MPEG audio data — it does not encode PCM
/// into Layer III bitstreams (that is a codec's job, out of scope for a
/// container/framing crate). [`Muxer::write_frame`] validates that `frame_body`'s
/// length matches what the header's bitrate/sample-rate/padding combination
/// requires, so a caller cannot silently write a frame that desyncs a real decoder.
#[derive(Debug, Clone, Copy)]
pub struct Muxer {
    header: FrameHeader,
    bitrate_index: u8,
    sample_rate_index: u8,
}

impl Muxer {
    /// Validate `header` (bitrate/sample rate must be standard Layer III values
    /// for `header.version`) and start a mux session.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "bitrate/sample-rate tables have 14/3 entries; the index always fits u8"
    )]
    pub fn new(header: FrameHeader) -> Result<Self, Error> {
        let bitrate_index = bitrate_table(header.version)
            .iter()
            .position(|&kbps| kbps == header.bitrate_kbps)
            .map_or_else(
                || Err(Error::UnsupportedBitrate(header.bitrate_kbps)),
                |i| Ok(i as u8 + 1), // table index 0 => header field value 1 (0 = "free format", unsupported)
            )?;
        let sample_rate_index = sample_rate_table(header.version)
            .iter()
            .position(|&rate| rate == header.sample_rate)
            .map_or_else(
                || Err(Error::UnsupportedSampleRate(header.sample_rate)),
                |i| Ok(i as u8),
            )?;
        Ok(Self {
            header,
            bitrate_index,
            sample_rate_index,
        })
    }

    /// Append one Layer III frame (4-byte header + `frame_body`) to `out`.
    ///
    /// `frame_body` must be exactly `header.frame_len(padding) - 4` bytes — the
    /// already-encoded Layer III payload for this bitrate/sample-rate/padding
    /// combination.
    pub fn write_frame(
        &self,
        frame_body: &[u8],
        padding: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let expected = self.header.frame_len(padding) - HEADER_LEN;
        if frame_body.len() != expected {
            return Err(Error::FrameBodyLengthMismatch {
                expected,
                actual: frame_body.len(),
            });
        }

        out.push(0xFF);
        out.push(0xE0 | (version_bits(self.header.version) << 3) | (LAYER_III_BITS << 1) | 1);
        out.push(
            (self.bitrate_index << 4) | (self.sample_rate_index << 2) | (u8::from(padding) << 1),
        );
        out.push((self.header.channel_mode.bits() << 6) | 0b0000_0100); // original=1, rest 0
        out.extend_from_slice(frame_body);
        Ok(())
    }
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod tests;
