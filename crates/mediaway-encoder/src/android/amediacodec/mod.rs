//! `AMediaCodec` encode session (Android only), built on the safe `ndk` wrapper.
//!
//! See [ADR-0001](../../adr/android/0001-ndk-amediacodec-h264-cpu-upload.md) for the binding
//! choice, scope, and the zero-compile-verification caveat.

// No raw FFI `unsafe` in this crate — see `crate` root doc comment / ADR-0001.
#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for AMediaCodec modules; not part of the public crate API"
)]

mod codec;
mod video;

pub(crate) use video::AmediaCodecVideoEncoder;
