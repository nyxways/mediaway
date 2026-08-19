//! `WebCodecs` encode backend (wasm32). CPU `VideoFrame` / `AudioData` path first.

#![forbid(unsafe_code)]

mod chunks;
mod config;
mod timestamp;

pub use chunks::{EncodedAudioChunks, EncodedVideoChunks};
pub use config::{WebAudioOpenConfig, WebVideoOpenConfig};

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(not(target_arch = "wasm32"))]
mod host;
#[cfg(not(target_arch = "wasm32"))]
pub use host::*;
