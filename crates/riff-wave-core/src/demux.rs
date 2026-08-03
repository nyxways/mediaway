//! RIFF/WAVE demux — parses a complete in-memory file.

#![forbid(unsafe_code)]

use bytes::Bytes;

use crate::error::Error;
use crate::types::{SampleFormat, WaveFormat};

/// Parse a complete RIFF/WAVE byte buffer into its format and raw PCM `data` payload.
///
/// RIFF has no fragmented/streamable profile in scope here (mirrors [`crate::Muxer`]'s
/// buffer-until-`finish()` design) — the whole file must be available up front.
/// Unknown chunks (`LIST`, `fact`, …) are skipped.
pub fn parse(data: &[u8]) -> Result<(WaveFormat, Bytes), Error> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(Error::NotRiffWave);
    }

    let mut format = None;
    let mut payload = None;
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let tag = &data[pos..pos + 4];
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(size).min(data.len());
        let body = &data[body_start..body_end];

        match tag {
            b"fmt " => format = Some(parse_fmt_chunk(body)?),
            b"data" => payload = Some(Bytes::copy_from_slice(body)),
            _ => {}
        }

        pos = body_end + (size % 2); // word-aligned: 1 pad byte after an odd-sized chunk
    }

    let format = format.ok_or(Error::MissingFmtChunk)?;
    let payload = payload.unwrap_or_default();
    Ok((format, payload))
}

fn parse_fmt_chunk(body: &[u8]) -> Result<WaveFormat, Error> {
    if body.len() < 16 {
        return Err(Error::TruncatedFmtChunk);
    }
    let tag = u16::from_le_bytes([body[0], body[1]]);
    let sample_format = SampleFormat::from_tag(tag).ok_or(Error::UnsupportedFormatTag(tag))?;
    let channels = u16::from_le_bytes([body[2], body[3]]);
    let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
    let bits_per_sample = u16::from_le_bytes([body[14], body[15]]);
    Ok(WaveFormat {
        sample_format,
        channels,
        sample_rate,
        bits_per_sample,
    })
}

#[cfg(test)]
#[path = "demux_tests.rs"]
mod tests;
