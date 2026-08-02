//! Decoded frame results exposed to JavaScript.
//!
//! Plain data (no `web-sys` types) so the same shape compiles on host and wasm32 — mirrors
//! `mediaway-encoder-web`'s `config.rs` split between shared config types and the
//! per-target `wasm.rs` / `host.rs` implementations.

#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;

/// Frames decoded via [`crate::decode_video_chunks`], one entry per output `VideoFrame`.
///
/// Each frame's luma (Y) plane is read back to a CPU buffer via `VideoFrame::copyTo` (wasm
/// build) so callers (Playwright / JS) can verify content without touching `web-sys` types.
#[wasm_bindgen]
pub struct DecodedVideoFrames {
    timestamps_us: Vec<f64>,
    luma_planes: Vec<Vec<u8>>,
}

impl DecodedVideoFrames {
    /// Build from parallel `(timestamp_us, luma_plane)` pairs, one per decoded frame.
    #[must_use]
    pub const fn new(timestamps_us: Vec<f64>, luma_planes: Vec<Vec<u8>>) -> Self {
        Self {
            timestamps_us,
            luma_planes,
        }
    }
}

#[wasm_bindgen]
impl DecodedVideoFrames {
    /// Number of decoded frames.
    #[wasm_bindgen(getter)]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "test/smoke frame counts are tiny"
    )]
    pub fn frame_count(&self) -> u32 {
        self.timestamps_us.len() as u32
    }

    /// `VideoFrame.timestamp` (microseconds) of the decoded frame at `index`.
    pub fn timestamp_us(&self, index: u32) -> f64 {
        self.timestamps_us[index as usize]
    }

    /// Tightly packed `width * height` luma (Y) plane bytes for the frame at `index`, read
    /// back via `VideoFrame::copyTo` and de-strided to a contiguous buffer.
    pub fn luma_plane(&self, index: u32) -> Vec<u8> {
        self.luma_planes[index as usize].clone() // clone: owned copy handed to JS caller
    }
}

#[cfg(test)]
#[path = "frames_tests.rs"]
mod tests;
