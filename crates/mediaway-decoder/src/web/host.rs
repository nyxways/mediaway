//! Host-target stubs so `cargo test --workspace` compiles without `WebCodecs`.

#![forbid(unsafe_code)]
#![allow(
    clippy::unused_async,
    reason = "wasm_bindgen async exports must match wasm API"
)]

use wasm_bindgen::prelude::*;

use crate::web::frames::DecodedVideoFrames;

/// Host build: `WebCodecs` unavailable.
#[cfg(feature = "video")]
#[wasm_bindgen]
pub async fn is_webcodecs_video_decode_supported(
    _codec: String,
    _width: u32,
    _height: u32,
) -> bool {
    false
}

/// Host build: returns an error (browser-only).
#[cfg(feature = "video")]
#[wasm_bindgen]
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors wasm decode_video_chunks signature"
)]
pub async fn decode_video_chunks(
    _codec: String,
    _width: u32,
    _height: u32,
    _description: Option<Vec<u8>>,
    _chunk_data: Vec<u8>,
    _chunk_offsets: Vec<u32>,
    _chunk_lengths: Vec<u32>,
    _chunk_timestamps_us: Vec<f64>,
    _chunk_is_key: Vec<u8>,
) -> Result<DecodedVideoFrames, JsValue> {
    Err(JsValue::from_str("wasm32 browser only"))
}
