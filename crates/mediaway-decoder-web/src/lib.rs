//! `WebCodecs` decode backend (wasm32). `EncodedVideoChunk` → `VideoFrame` path.

#![forbid(unsafe_code)]

mod frames;
mod timestamp;

pub use frames::DecodedVideoFrames;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(not(target_arch = "wasm32"))]
mod host;
#[cfg(not(target_arch = "wasm32"))]
pub use host::*;
