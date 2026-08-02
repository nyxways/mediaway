//! Host-target stubs so `cargo test --workspace` compiles without `WebCodecs`.

#![forbid(unsafe_code)]
#![allow(
    clippy::unused_async,
    reason = "wasm_bindgen async exports must match wasm API"
)]

use wasm_bindgen::prelude::*;

use crate::chunks::EncodedVideoChunks;
use crate::config::{WebAudioOpenConfig, WebVideoOpenConfig};

/// Host build: `WebCodecs` unavailable.
#[cfg(all(feature = "audio", feature = "video"))]
#[wasm_bindgen]
pub async fn is_webcodecs_av_supported() -> bool {
    false
}

/// Host build: returns an error (browser-only).
#[cfg(all(feature = "audio", feature = "video"))]
#[wasm_bindgen]
pub async fn webcodecs_av_fmp4_smoke() -> Result<Vec<u8>, JsValue> {
    Err(JsValue::from_str("wasm32 browser only"))
}

/// Host build: `WebGPU` / `WebCodecs` unavailable.
#[cfg(feature = "video")]
#[wasm_bindgen]
pub async fn is_webgpu_video_frame_supported() -> bool {
    false
}

/// Host build: returns an error (browser-only).
#[cfg(feature = "video")]
#[wasm_bindgen]
pub async fn webcodecs_gpu_video_fmp4_smoke() -> Result<Vec<u8>, JsValue> {
    Err(JsValue::from_str("wasm32 browser only"))
}

/// Host build: returns an error (browser-only).
#[cfg(feature = "video")]
#[wasm_bindgen]
pub async fn encode_video_frames(
    _codec: String,
    _width: u32,
    _height: u32,
    _bitrate_bps: u32,
    _lumas: Vec<u8>,
    _timestamps_us: Vec<f64>,
) -> Result<EncodedVideoChunks, JsValue> {
    Err(JsValue::from_str("wasm32 browser only"))
}

/// Map facade video config to a browser label (smoke / docs).
#[cfg(feature = "video")]
#[wasm_bindgen]
pub fn video_config_label(_config: &WebVideoOpenConfig) -> String {
    "h264".to_string()
}

/// Map facade audio config to a browser label (smoke / docs).
#[cfg(feature = "audio")]
#[wasm_bindgen]
pub fn audio_config_label(_config: &WebAudioOpenConfig) -> String {
    "aac".to_string()
}

/// Demux packet count from fMP4 bytes (smoke helper for Playwright).
#[wasm_bindgen]
pub fn fmp4_packet_count(bytes: &[u8]) -> u32 {
    let mut demux = iso_bmff::Demuxer::new();
    demux.push_bytes(bytes);
    let mut n = 0u32;
    while demux.poll_packet().is_some() {
        n += 1;
    }
    n
}
