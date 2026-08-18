//! Hardware-accelerated video decoding **facade**.
//!
//! - **Low-level:** [`VideoDecoder`] — OS sessions in `mediaway-decoder-<platform>`
//!   ([ADR-0001](../adr/0001-decoder-traits.md), [ADR-0002](../adr/0002-facade-platform-boundary.md)).
//! - Windows Zero-Copy: `mediaway_decoder_windows::WindowsVideoDecoder::open`.

#![allow(unsafe_code)]

mod audio;
pub mod capability;
mod error;
mod video;

pub use audio::AudioDecoder;
pub use audio::sw_opus::SwOpusAudioDecoder;
pub use error::DecodeError;
pub use video::{VideoDecoder, VideoDecoderConfig, VideoOutputPreference};

// ── merged platform/domain modules (ADR-0021) ──
pub mod apple;
pub mod linux;
#[cfg(not(target_family = "wasm"))] // Vulkan Video — desktop only (vulkanalia/libloading)
pub mod vulkan;
pub mod web;
pub mod windows;
