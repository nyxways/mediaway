//! Browser-facing open configs for `WebCodecs` (wasm).

#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;

/// Video encode parameters exposed to JavaScript (CPU NV12 path).
#[wasm_bindgen]
pub struct WebVideoOpenConfig {
    width: u32,
    height: u32,
    bitrate_bps: u32,
}

#[wasm_bindgen]
impl WebVideoOpenConfig {
    /// Default 64×64 smoke size.
    #[wasm_bindgen(constructor)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen constructor")]
    pub fn new(width: u32, height: u32, bitrate_bps: u32) -> Self {
        Self {
            width,
            height,
            bitrate_bps,
        }
    }

    /// Width in pixels.
    #[wasm_bindgen(getter)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen getter")]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[wasm_bindgen(getter)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen getter")]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Target bitrate (bps).
    #[wasm_bindgen(getter)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen getter")]
    pub fn bitrate_bps(&self) -> u32 {
        self.bitrate_bps
    }
}

/// Audio encode parameters exposed to JavaScript.
#[wasm_bindgen]
pub struct WebAudioOpenConfig {
    sample_rate: u32,
}

#[wasm_bindgen]
impl WebAudioOpenConfig {
    /// Default 48 kHz stereo path.
    #[wasm_bindgen(constructor)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen constructor")]
    pub fn new(sample_rate: u32) -> Self {
        Self { sample_rate }
    }

    /// Sample rate (Hz).
    #[wasm_bindgen(getter)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen getter")]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
