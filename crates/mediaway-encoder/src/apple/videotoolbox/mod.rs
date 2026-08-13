//! `VTCompressionSession` encode session (Apple only), built on `objc2-video-toolbox` /
//! `objc2-core-video` / `objc2-core-media` / `objc2-core-foundation`.
//!
//! See [ADR-0001](../../adr/apple/0001-videotoolbox-h264-cpu-upload.md) for the binding choice,
//! scope, and the zero-compile-verification caveat.
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for VideoToolbox modules; not part of the public crate API"
)]

mod codec;
mod video;

pub(crate) use video::VideoToolboxVideoEncoder;
