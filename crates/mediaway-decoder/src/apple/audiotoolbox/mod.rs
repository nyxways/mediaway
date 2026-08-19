//! `AudioConverter` AAC/Opus decode sessions (Apple only), built on `objc2-audio-toolbox` /
//! `objc2-core-audio-types`.
//!
//! See [ADR-0004](../../adr/apple/0004-audiotoolbox-aac-decode.md) (AAC — requires a
//! decompression magic cookie) and [ADR-0005](../../adr/apple/0005-audiotoolbox-opus-decode.md)
//! (Opus — no config record needed) for the pull-based `AudioConverterFillComplexBuffer`
//! callback contract and the zero-compile-verification caveat.
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for AudioToolbox modules; not part of the public crate API — mirrors videotoolbox/mod.rs"
)]

mod aac;
mod opus;

pub use aac::{AacDecoder, AacDecoderConfig};
pub use opus::{OpusDecoder, OpusDecoderConfig};
