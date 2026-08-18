//! `VTDecompressionSession` decode session (Apple only), built on `objc2-video-toolbox` /
//! `objc2-core-video` / `objc2-core-media` / `objc2-core-foundation`.
//!
//! See [ADR-0001](../../../adr/apple/0001-videotoolbox-h264-cpu-out.md) for the binding choice,
//! scope, and the zero-compile-verification caveat.
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for VideoToolbox modules; not part of the public crate API — mirrors linux/vaapi/mod.rs and encoder::apple::videotoolbox::mod.rs"
)]

// Real `objc2-*` calls — Apple targets only (the `objc2-video-toolbox`/`objc2-core-media`/
// `objc2-core-video` crates are not dependencies on other targets, see Cargo.toml).
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod video;

// `codec`'s helpers are pure (no `objc2-*` import) so they build and unit-test on any host —
// compiled for real use on Apple targets, and additionally under `cfg(test)` so
// `codec_tests.rs` runs on this crate's non-Apple CI/dev hosts too (there is otherwise nothing
// in this module for a non-Apple, non-test build to compile, so nothing would be "dead code").
#[cfg(any(target_os = "macos", target_os = "ios", test))]
mod codec;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use video::VideoToolboxVideoDecoder;
