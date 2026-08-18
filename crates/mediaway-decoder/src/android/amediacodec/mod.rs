//! `AMediaCodec` decode session (Android only) plus its host-testable pure helpers.
//!
//! See [ADR android/0001](../../adr/android/0001-ndk-amediacodec-h264-cpu-out.md) for the
//! binding choice, scope, and the zero-compile/zero-runtime-verification caveat.
//!
//! `nv12`/`csd` are pure byte-manipulation helpers with no `ndk` dependency — compiled under
//! `cfg(any(target_os = "android", test))` so their unit tests run on any host (this crate's
//! dev environment has no Android NDK). `codec`/`video` touch the real `MediaCodec`/
//! `MediaFormat` API and stay `target_os = "android"`-only, per this backend's "zero compile
//! verification" caveat — they are not host-testable.

// No raw FFI `unsafe` in this crate — see `crate` root doc comment / ADR android/0001.
#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for AMediaCodec modules; not part of the public crate API"
)]

#[cfg(target_os = "android")]
mod codec;
#[cfg(any(target_os = "android", test))]
mod csd;
#[cfg(any(target_os = "android", test))]
mod nv12;
#[cfg(target_os = "android")]
mod video;

#[cfg(target_os = "android")]
pub(crate) use video::AmediaCodecVideoDecoder;
