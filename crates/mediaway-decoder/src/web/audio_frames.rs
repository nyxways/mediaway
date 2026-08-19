//! Decoded audio frame results exposed to JavaScript.
//!
//! Plain data (no `web-sys` types) so the same shape compiles on host and wasm32 — mirrors
//! `frames.rs`'s split between shared result types and the per-target `wasm.rs` / `host.rs`
//! implementations.

#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;

/// Frames decoded via [`crate::web::decode_audio_chunks`], one entry per output `AudioData`.
///
/// Each frame's samples are read back to a flat, channel-interleaved `f32` buffer (see
/// [`crate::web::decode_audio_chunks`]'s doc comment for the planar-vs-interleaved readback
/// shape) so callers (Playwright / JS) can verify content without touching `web-sys` types —
/// same "wasm-module-boundary" reasoning [`crate::web::DecodedVideoFrames`] already documents
/// for video.
#[wasm_bindgen]
pub struct DecodedAudioData {
    timestamps_us: Vec<f64>,
    sample_counts: Vec<u32>,
    channel_counts: Vec<u32>,
    samples: Vec<Vec<f32>>,
}

impl DecodedAudioData {
    /// Build from parallel `(timestamp_us, sample_count, channel_count, samples)` tuples, one
    /// per decoded `AudioData`. `sample_count` is `AudioData.numberOfFrames` (samples per
    /// channel); `samples` is channel-interleaved, `sample_count * channel_count` long.
    #[must_use]
    pub const fn new(
        timestamps_us: Vec<f64>,
        sample_counts: Vec<u32>,
        channel_counts: Vec<u32>,
        samples: Vec<Vec<f32>>,
    ) -> Self {
        Self {
            timestamps_us,
            sample_counts,
            channel_counts,
            samples,
        }
    }
}

#[wasm_bindgen]
impl DecodedAudioData {
    /// Number of decoded `AudioData` chunks.
    #[wasm_bindgen(getter)]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "test/smoke chunk counts are tiny"
    )]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "#[wasm_bindgen] does not support const fn"
    )]
    pub fn chunk_count(&self) -> u32 {
        self.timestamps_us.len() as u32
    }

    /// `AudioData.timestamp` (microseconds) of the decoded chunk at `index`.
    pub fn timestamp_us(&self, index: u32) -> f64 {
        self.timestamps_us[index as usize]
    }

    /// `AudioData.numberOfFrames` (samples per channel) of the chunk at `index`.
    pub fn sample_count(&self, index: u32) -> u32 {
        self.sample_counts[index as usize]
    }

    /// `AudioData.numberOfChannels` of the chunk at `index`.
    pub fn channel_count(&self, index: u32) -> u32 {
        self.channel_counts[index as usize]
    }

    /// Channel-interleaved `f32` samples (`sample_count(index) * channel_count(index)` long)
    /// for the chunk at `index`.
    pub fn samples(&self, index: u32) -> Vec<f32> {
        self.samples[index as usize].clone() // clone: owned copy handed to JS caller
    }
}

#[cfg(test)]
#[path = "audio_frames_tests.rs"]
mod tests;
