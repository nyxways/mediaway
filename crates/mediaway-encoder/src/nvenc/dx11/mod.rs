//! Windows D3D11 + NVENC session backend (`#[cfg(windows)]` only — see crate root docs).

#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for the D3D11/NVENC modules; not part of the public crate API"
)]

mod device;
mod video;

pub(crate) use video::NvencSession;
