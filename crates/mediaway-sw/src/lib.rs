//! Pure Rust sans-io software codec fallbacks (no C codec FFI).
//!
//! No C codec FFI (`OpenH264`, `libvpx`, …). Codec logic lives in Rust cores
//! (or reviewed Rust deps such as `rav1e`) behind sans-io adapters. Stage 5 of the
//! workspace roadmap; see `docs/roadmap.md` for this crate's own staging.
//!
//! Current scope: [`h264`] Annex-B/AVCC NAL unit framing, SPS/PPS header parsing, and a
//! Baseline/CAVLC/I-slice single-frame pixel decode loop (`I_16x16`/`I_PCM` macroblocks
//! only, no deblocking filter); [`pcm`] raw PCM passthrough encode/decode; [`av1`] AV1
//! encode via `rav1e`. See `adr/0001-h264-baseline-decoder-first.md` for the H.264 decode
//! staging plan, `adr/0003-cavlc-i-slice-first-decode.md` for this decode loop's exact
//! scope cuts, and `adr/0002-rav1e-av1-encode.md` for the AV1 adapter scope.
//!
//! # Cargo features
//!
//! Each codec family is a feature so a consumer pays only for what it uses
//! (`adr/0004-codec-feature-split.md`): `av1` (pulls `rav1e`), `h264`, `opus` (pulls
//! `unsafe-libopus`), `apm` / `vad` (pull `sonora*`). `default = ["av1", "h264", "opus"]`
//! (the behavior before the split); `full` enables everything. [`pcm`] is always compiled.

#![allow(unsafe_code)]

#[cfg(feature = "av1")]
pub mod av1;
#[cfg(feature = "h264")]
pub mod h264;
pub mod pcm;

// ── merged platform/domain modules (ADR-0021) ──
pub mod apm;
#[cfg(feature = "opus")]
pub mod opus;
