//! Audio enhancement (echo cancellation, noise suppression, gain control,
//! voice activity detection) — thin Mediaway-typed adapter over the pure-Rust
//! [`sonora`](https://crates.io/crates/sonora) WebRTC Audio Processing port.
//!
//! Meant to sit right after microphone capture
//! (`mediaway_device::AudioCapture::poll_frame`), before anything else
//! touches the signal — **not** a hook on `mediaway_pipeline::EncodeSession`,
//! which has no audio track support today. See
//! `adr/0001-sonora-audio-processing-adoption.md` for the full design
//! rationale: license verdict, crate placement, the `AudioProcessor` /
//! `VoiceActivityDetector` render/capture push·poll shape (deliberately
//! **not** a `FrameFilter`-parallel single-stream trait), the panic-safety
//! posture, and the VAD i16-scale integration caveat.
//!
//! [`AudioProcessor`] (`apm` feature — AEC3 + NS + AGC2, via
//! `sonora::AudioProcessing`) and [`VoiceActivityDetector`] (`vad` feature —
//! RNN VAD, via `sonora_agc2::vad_wrapper`) are independent, concrete types,
//! each usable standalone. Both catch `sonora`/`sonora-agc2` panics and
//! permanently disable the offending instance rather than propagate — see
//! `is_disabled()` on each type and [`ApmError::BackendPanicked`].
#![forbid(unsafe_code)]

mod error;
#[cfg(any(feature = "apm", feature = "vad"))]
mod pcm;
#[cfg(feature = "apm")]
mod processor;
#[cfg(feature = "vad")]
mod vad;

pub use error::ApmError;
#[cfg(feature = "apm")]
pub use processor::{AudioProcessor, AudioStreamFormat};
#[cfg(feature = "vad")]
pub use vad::VoiceActivityDetector;

/// `sonora`'s top-level processing configuration — passed to
/// [`AudioProcessor::open`]. All components (echo canceller, noise
/// suppression, gain controller, …) are disabled (`None`) by default;
/// enable them via the [`config`] module's types, e.g.
/// `ApmConfig { echo_canceller: Some(config::EchoCanceller::default()), ..Default::default() }`.
#[cfg(feature = "apm")]
pub use sonora::Config as ApmConfig;

/// Re-export of `sonora`'s processing configuration module
/// (`EchoCanceller`, `NoiseSuppression`, `GainController2`, …) — construct
/// an [`ApmConfig`] using these types directly. This crate defines no
/// parallel config surface of its own; see
/// `adr/0001-sonora-audio-processing-adoption.md` § 3 for why (thin adapter,
/// not a custom abstraction over `sonora`).
#[cfg(feature = "apm")]
pub use sonora::config;
