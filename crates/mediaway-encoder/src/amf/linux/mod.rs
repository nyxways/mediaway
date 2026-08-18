//! AMD AMF encode session (Linux `x86_64` only), built on `shiguredo_amf`.
//!
//! See [ADR-0002](../../adr/amf/0002-amf-linux-shiguredo-amf-h264-cpu-upload.md) for the
//! binding choice, scope, and the zero-hardware-verification caveat.

// `shiguredo_amf`'s `Surface`/`Plane` write API is a raw-pointer, `unsafe`-write API
// (`Plane::get_native() -> *mut c_void`), unlike `linux::vaapi`'s sibling module which
// stays entirely behind `cros-libva`'s safe wrapper. See `session.rs` for the localized
// `unsafe` blocks and their `// SAFETY:` comments.
#![allow(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for AMF modules; not part of the public crate API"
)]

mod codec;
mod session;

pub(crate) use session::AmfSession;
