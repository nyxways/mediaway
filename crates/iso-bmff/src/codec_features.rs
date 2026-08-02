//! Cargo feature checks for audio vs video codec paths.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "crate-private helpers for mux/demux"
)]

use crate::error::Error;
use crate::types::Codec;

/// Whether `codec` is enabled in this build (`audio` / `video` features).
#[must_use]
const fn codec_enabled(codec: Codec) -> bool {
    match codec {
        Codec::Aac | Codec::Opus => cfg!(feature = "audio"),
        Codec::H264 | Codec::Hevc | Codec::Av1 | Codec::Vp9 | Codec::WebVtt | Codec::Tx3g => {
            cfg!(feature = "video")
        }
    }
}

/// Returns [`Error::InvalidPacket`] when `codec` is disabled for this build.
pub(crate) const fn check_codec(codec: Codec) -> Result<(), Error> {
    if codec_enabled(codec) {
        Ok(())
    } else {
        Err(Error::InvalidPacket)
    }
}
