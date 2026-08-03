//! RIFF/WAVE mux (`fmt ` + `data` chunks).

#![forbid(unsafe_code)]

use crate::types::WaveFormat;

/// Builds a single RIFF/WAVE file from pushed PCM samples.
///
/// Unlike `iso-bmff`'s incrementally-flushable fragmented output, RIFF's `RIFF` and
/// `data` chunk sizes must be known before the header can be written — there is no
/// fragmented/streamable RIFF profile in scope here. Samples are buffered internally
/// until [`Muxer::finish`] is called.
#[derive(Debug, Clone)]
pub struct Muxer {
    format: WaveFormat,
    samples: Vec<u8>,
}

impl Muxer {
    /// Start a new mux session for `format`.
    #[must_use]
    pub const fn new(format: WaveFormat) -> Self {
        Self {
            format,
            samples: Vec::new(),
        }
    }

    /// Append raw interleaved PCM bytes (already encoded per `format`).
    pub fn push_samples(&mut self, pcm: &[u8]) {
        self.samples.extend_from_slice(pcm);
    }

    /// Finalize and return the complete RIFF/WAVE byte stream.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        let data_len = u32::try_from(self.samples.len()).unwrap_or(u32::MAX);
        let riff_len = 4 + (8 + 16) + (8 + data_len);

        let mut out = Vec::with_capacity(12 + 24 + 8 + self.samples.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&riff_len.to_le_bytes());
        out.extend_from_slice(b"WAVE");

        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&self.format.sample_format.tag().to_le_bytes());
        out.extend_from_slice(&self.format.channels.to_le_bytes());
        out.extend_from_slice(&self.format.sample_rate.to_le_bytes());
        out.extend_from_slice(&self.format.byte_rate().to_le_bytes());
        out.extend_from_slice(&self.format.block_align().to_le_bytes());
        out.extend_from_slice(&self.format.bits_per_sample.to_le_bytes());

        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(&self.samples);
        if data_len % 2 == 1 {
            out.push(0); // RIFF chunks are word-aligned; pad odd-sized data chunks.
        }
        out
    }
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod tests;
