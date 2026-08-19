//! Encoded chunk results exposed to JavaScript.
//!
//! Plain data (no `web-sys` types) so the same shape compiles on host and wasm32 — mirrors
//! [`crate::web::config`]'s split between shared config types and the per-target `wasm.rs` /
//! `host.rs` implementations.

#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;

/// Chunks produced by [`crate::web::encode_video_frames`], one entry per output
/// `EncodedVideoChunk`.
///
/// Exposed as flattened getters (not a `Vec` of `web-sys` objects) because wasm-bindgen
/// values are scoped to a single wasm module/instance — a consumer compiled into a
/// *different* wasm module (e.g. `mediaway-decoder-web`) can only read this data back as
/// plain values through calls like these, not as a shared Rust/JS type. See
/// `tools/e2e-web/tests/decode-trim-splice.spec.ts`.
#[wasm_bindgen]
pub struct EncodedVideoChunks {
    timestamps_us: Vec<f64>,
    keyframes: Vec<bool>,
    payloads: Vec<Vec<u8>>,
    description: Option<Vec<u8>>,
}

impl EncodedVideoChunks {
    /// Build from parallel `(timestamp_us, is_keyframe, payload)` triples, one per chunk, plus
    /// the `VideoDecoderConfig.description` captured from the encoder (if any — see
    /// [`Self::description`]).
    #[must_use]
    pub const fn new(
        timestamps_us: Vec<f64>,
        keyframes: Vec<bool>,
        payloads: Vec<Vec<u8>>,
        description: Option<Vec<u8>>,
    ) -> Self {
        Self {
            timestamps_us,
            keyframes,
            payloads,
            description,
        }
    }
}

#[wasm_bindgen]
impl EncodedVideoChunks {
    /// Number of encoded chunks.
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
        self.payloads.len() as u32
    }

    /// `EncodedVideoChunk.timestamp` (microseconds) of the chunk at `index`.
    pub fn timestamp_us(&self, index: u32) -> f64 {
        self.timestamps_us[index as usize]
    }

    /// Whether the chunk at `index` is a keyframe (`EncodedVideoChunkType::Key`).
    pub fn is_key(&self, index: u32) -> bool {
        self.keyframes[index as usize]
    }

    /// Raw encoded payload bytes of the chunk at `index`.
    pub fn data(&self, index: u32) -> Vec<u8> {
        self.payloads[index as usize].clone() // clone: owned copy handed to JS caller
    }

    /// `VideoDecoderConfig.description` captured from the encoder's `EncodedVideoChunkMetadata`
    /// (e.g. H.264's out-of-band SPS/PPS `avcC` record) — `None` for codecs that don't need
    /// one (VP8/VP9/AV1 are normally self-describing in-band). Pass straight through to
    /// `mediaway-decoder-web`'s `decode_video_chunks` `description` parameter.
    #[wasm_bindgen(getter)]
    pub fn description(&self) -> Option<Vec<u8>> {
        self.description.clone() // clone: owned copy handed to JS caller
    }
}

/// Chunks produced by [`crate::web::encode_audio_buffer`], one entry per output
/// `EncodedAudioChunk`.
///
/// Flattened getters for the same wasm-module-boundary reason as [`EncodedVideoChunks`] (see
/// its doc comment) — no keyframe/description fields, since `WebCodecs` audio chunks carry
/// neither an in-band keyframe distinction ([`crate::web::encode_audio_buffer`] always
/// produces independently-decodable output) nor an out-of-band decoder description in this
/// crate's Opus/AAC smoke paths.
#[wasm_bindgen]
pub struct EncodedAudioChunks {
    timestamps_us: Vec<f64>,
    payloads: Vec<Vec<u8>>,
}

impl EncodedAudioChunks {
    /// Build from parallel `(timestamp_us, payload)` pairs, one per chunk.
    #[must_use]
    pub const fn new(timestamps_us: Vec<f64>, payloads: Vec<Vec<u8>>) -> Self {
        Self {
            timestamps_us,
            payloads,
        }
    }
}

#[wasm_bindgen]
impl EncodedAudioChunks {
    /// Number of encoded chunks.
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
        self.payloads.len() as u32
    }

    /// `EncodedAudioChunk.timestamp` (microseconds) of the chunk at `index`.
    pub fn timestamp_us(&self, index: u32) -> f64 {
        self.timestamps_us[index as usize]
    }

    /// Raw encoded payload bytes of the chunk at `index`.
    pub fn data(&self, index: u32) -> Vec<u8> {
        self.payloads[index as usize].clone() // clone: owned copy handed to JS caller
    }
}

#[cfg(test)]
#[path = "chunks_tests.rs"]
mod tests;
