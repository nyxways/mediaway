//! `AudioConverter` AAC encode session (Apple only), built on `objc2-audio-toolbox` /
//! `objc2-core-audio-types`.
//!
//! See [ADR-0004](../../adr/apple/0004-audiotoolbox-aac-encode.md) for the binding choice, the
//! pull-based `AudioConverterFillComplexBuffer` callback contract, and the zero-compile-
//! verification caveat.
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for AudioToolbox modules; not part of the public crate API — mirrors videotoolbox/mod.rs"
)]

mod aac;

pub(crate) use aac::AacEncoder;
