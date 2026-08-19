//! VA-API encode session (Linux only), built on the safe `cros-libva` wrapper.
//!
//! See [ADR-0001](../../adr/0001-vaapi-cros-libva-h264-cpu-upload.md) for the binding
//! choice, scope, and the zero-hardware-verification caveat.
//!
//! Every raw VA-API call goes through `cros-libva`'s safe wrapper layer, same as ADR-0001 — this
//! module is `#[allow(unsafe_code)]`, not `#[forbid]`, only because `dmabuf.rs`
//! (ADR-0003-vaapi-dmabuf-zero-copy-input) must reconstruct a [`std::os::fd::BorrowedFd`] from a
//! caller-supplied raw fd number (`BorrowedFd::borrow_raw` is an `unsafe fn` in `std` itself —
//! that reconstruction step cannot be expressed safely). Every `unsafe` block carries a
//! `// SAFETY:` comment; `codec.rs`/`gop.rs`/`video.rs` still write none.
#![allow(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for VA-API modules; not part of the public crate API"
)]

mod codec;
mod dmabuf;
mod gop;
mod video;

pub(crate) use video::VaapiVideoEncoder;
