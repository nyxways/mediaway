//! VA-API decode helpers (Linux, `cros-libva`).

#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for VA-API modules; not part of the public crate API — mirrors mediaway-decoder-windows's wmf/mod.rs"
)]

mod codec;
mod dpb;
mod h264;
mod nv12;
mod pps;
mod slice;
mod sps;

pub(crate) use h264::VaapiH264Decoder;
