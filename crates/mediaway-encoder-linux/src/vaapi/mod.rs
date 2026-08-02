//! VA-API encode session (Linux only), built on the safe `cros-libva` wrapper.
//!
//! See [ADR-0001](../../adr/0001-vaapi-cros-libva-h264-cpu-upload.md) for the binding
//! choice, scope, and the zero-hardware-verification caveat.

// No raw FFI `unsafe` in this crate — see `crate` root doc comment / ADR-0001.
#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for VA-API modules; not part of the public crate API"
)]

mod codec;
mod video;

pub(crate) use video::VaapiVideoEncoder;
