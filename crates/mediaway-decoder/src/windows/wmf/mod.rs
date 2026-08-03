//! Windows Media Foundation decode helpers.

#![allow(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) graph for MF modules; not part of the public crate API"
)]

#[cfg(feature = "video")]
mod codec;
#[cfg(feature = "video")]
mod cpu;
#[cfg(feature = "video")]
mod dx11;
#[cfg(feature = "video")]
mod h264;
#[cfg(feature = "audio")]
mod opus;
mod runtime;
#[cfg(feature = "video")]
mod shared;
#[cfg(feature = "video")]
mod video_cpu;

#[cfg(feature = "video")]
pub(crate) use h264::WmfH264Decoder;
#[cfg(feature = "video")]
pub(crate) use video_cpu::WmfMultiCodecCpuDecoder;
