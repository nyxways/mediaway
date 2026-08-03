//! ADTS frame writer — one 7-byte header (no CRC) per raw AAC payload.

#![forbid(unsafe_code)]

use crate::error::Error;
use crate::types::{AdtsConfig, sampling_frequency_index};

const HEADER_LEN: usize = 7;
const MAX_FRAME_LEN: usize = 0x1FFF; // 13-bit aac_frame_length field
const BUFFER_FULLNESS_UNKNOWN: u16 = 0x7FF; // VBR / not indicated

/// Writes ADTS frames for a fixed `AdtsConfig`.
///
/// Unlike `iso-bmff`'s box-based mux, ADTS has no container-level header at all —
/// each call appends one self-contained frame directly to `out`, so there is no
/// `finish()` step.
#[derive(Debug, Clone, Copy)]
pub struct Muxer {
    config: AdtsConfig,
    sfi: u8,
}

impl Muxer {
    /// Validate `config` (sample rate must be a standard ADTS rate) and start a mux session.
    pub fn new(config: AdtsConfig) -> Result<Self, Error> {
        let sfi = sampling_frequency_index(config.sample_rate)
            .ok_or(Error::UnsupportedSampleRate(config.sample_rate))?;
        Ok(Self { config, sfi })
    }

    /// Append one ADTS frame (7-byte header + `raw_aac`) to `out`.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "every cast operand is bit-masked to fit u8 immediately before the cast"
    )]
    pub fn write_frame(&self, raw_aac: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
        let frame_len = HEADER_LEN + raw_aac.len();
        if frame_len > MAX_FRAME_LEN {
            return Err(Error::FrameTooLarge(frame_len));
        }
        let profile = self.config.profile.bits();
        let channels = self.config.channels & 0x07;
        let fullness = usize::from(BUFFER_FULLNESS_UNKNOWN);

        out.push(0xFF);
        out.push(0xF1); // MPEG-4 (ID=0), layer=00, protection_absent=1 (no CRC)
        out.push((profile << 6) | (self.sfi << 2) | (channels >> 2));
        out.push(((channels & 0x03) << 6) | ((frame_len >> 11) & 0x03) as u8);
        out.push(((frame_len >> 3) & 0xFF) as u8);
        out.push((((frame_len & 0x07) as u8) << 5) | ((fullness >> 6) & 0x1F) as u8);
        out.push(((fullness & 0x3F) as u8) << 2); // low 6 bits of fullness + 2-bit block count (0 = 1 block)
        out.extend_from_slice(raw_aac);
        Ok(())
    }
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod tests;
