//! Windows Media Foundation encode helpers.

#![allow(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for MF modules; not part of the public crate API"
)]

#[cfg(feature = "audio")]
mod aac;
#[cfg(feature = "video")]
mod codec;
#[cfg(feature = "video")]
mod dx11;
mod runtime;
mod shared;
#[cfg(feature = "video")]
mod video;

#[cfg(feature = "audio")]
pub(crate) use aac::WmfAacEncoder;
#[cfg(feature = "video")]
pub(crate) use video::WmfVideoEncoder;
