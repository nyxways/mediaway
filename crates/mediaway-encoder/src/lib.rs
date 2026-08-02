//! Hardware-accelerated video and audio encoding **facade**.
//!
//! - **Low-level:** [`VideoEncoder`] / [`AudioEncoder`] — OS sessions in
//!   `mediaway-encoder-<platform>` ([ADR-0001](../adr/0001-encoder-traits.md),
//!   [ADR-0002](../adr/0002-facade-platform-boundary.md)).
//! - **High-level types:** [`auto`] — [`auto::EncodePathClass`] / policy / config
//!   ([ADR-0003](../adr/0003-auto-encode.md)). Windows session:
//!   `mediaway_encoder_windows::auto::AutoVideoEncoder::open`.
//!
//! Enable [`audio`](crate#features) / [`video`](crate#features) features for slim builds
//! ([ADR-0004](../adr/0004-av-feature-gates.md)).
//!
//! A future `mediaway-codec` umbrella may re-export this crate + `mediaway-decoder`.
//! Backends (planned): WMF, `VideoToolbox`, `MediaCodec`, `WebCodecs`, Vulkan Video.

#![forbid(unsafe_code)]

#[cfg(all(not(feature = "audio"), not(feature = "video")))]
compile_error!("enable at least one of `audio` or `video` features on mediaway-encoder");

#[cfg(feature = "video")]
pub mod auto;
#[cfg(feature = "video")]
pub mod capability;

#[cfg(feature = "audio")]
mod audio;
mod error;
#[cfg(feature = "video")]
mod video;

#[cfg(feature = "audio")]
pub use audio::{AudioEncoder, AudioEncoderConfig};
pub use error::EncodeError;
#[cfg(feature = "video")]
pub use video::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};
