//! Pure Rust Opus encode/decode, isolated unsafe boundary.
//!
//! Wraps [`unsafe-libopus`](https://crates.io/crates/unsafe-libopus) (BSD-3-Clause,
//! `c2rust`-transpiled libopus 1.3.1) behind a safe RAII [`encoder::OpusEncoder`] /
//! [`decoder::OpusDecoder`] surface. `unsafe-libopus`'s own public functions are `unsafe fn`
//! (C-shaped create/encode/decode/destroy on raw pointers) — unlike `mediaway-sw`'s `rav1e`
//! dependency, whose public API is fully safe. That is why this crate exists separately from
//! `mediaway-sw` (which stays `#![forbid(unsafe_code)]`, no exceptions) rather than as a
//! module inside it. See `adr/0001-unsafe-libopus-encode-decode.md` for the full design
//! rationale, dependency review, and public API shape.
//!
//! Shaped like the push/poll session pattern used elsewhere in the workspace —
//! [`encoder::OpusEncoder`] mirrors `mediaway_encoder::AudioEncoder`'s method names
//! (`stream_info` / `push_frame` / `poll_packet` / `flush`), and [`decoder::OpusDecoder`]
//! mirrors `mediaway-decoder-windows`'s `WmfOpusDecoder` session shape (`push_packet` /
//! `poll_frame` / `flush`) — without depending on `mediaway-encoder`/`mediaway-decoder` to
//! avoid an unwanted circular/inflated dependency graph for a leaf codec crate. Neither
//! session `impl`s those traits directly; a factory in the encoder/decoder facades can wire
//! this crate in as a fallback later without this crate needing to depend on them first.
//!
//! Only [`mediaway_common::SampleFormat::F32`] PCM is accepted (`opus_encode_float` /
//! `opus_decode_float`) and only exact Opus frame sizes (no internal re-buffering) — see
//! [`config::OpusEncoderConfig::time_base`] and [`error::OpusError::FrameSizeMismatch`].
//!
//! **Status: encode + decode implemented.** See `docs/roadmap.md` for remaining wiring work
//! (an `AudioDecoder` trait does not exist in `mediaway-decoder` yet to implement against).

#![allow(clippy::too_long_first_doc_paragraph)]
// crate-root doc became module doc (ADR-0021 merge)
// This crate's entire purpose is a safe wrapper around `unsafe-libopus`'s C-shaped
// create/encode/decode/destroy/ctl API — every module here touches the raw pointer boundary,
// so the allow lives at the crate root rather than being sprinkled per-module (mirrors
// `vpl-sys`, another single-purpose FFI-shaped core). Every `unsafe` block below carries a
// `// SAFETY:` comment per docs/conventions/code-style.md § unsafe.
#![allow(unsafe_code)]

pub mod config;
pub mod decoder;
pub mod encoder;
pub mod error;
