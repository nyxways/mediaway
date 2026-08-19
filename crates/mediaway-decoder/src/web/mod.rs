//! `WebCodecs` decode backend (wasm32). `EncodedVideoChunk` → `VideoFrame` and
//! `EncodedAudioChunk` → `AudioData` paths.

#![forbid(unsafe_code)]

mod audio_frames;
mod frames;
mod timestamp;

pub use audio_frames::DecodedAudioData;
pub use frames::DecodedVideoFrames;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(not(target_arch = "wasm32"))]
mod host;
#[cfg(not(target_arch = "wasm32"))]
pub use host::*;
