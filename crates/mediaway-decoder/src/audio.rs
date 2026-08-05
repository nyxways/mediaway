//! Audio decode [`AudioDecoder`] trait.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "sw_opus is pub for the crate-root `pub use` re-export below; the `audio` \
              module itself stays private, same pattern as `windows::wmf`"
)]

pub mod sw_opus;
#[cfg(test)]
mod sw_opus_tests;

use crate::error::DecodeError;
use mediaway_common::{AudioFrame, Packet, StreamInfo};

/// Streaming hardware (or backend) audio decoder.
///
/// Push packets, then [`poll_frame`](AudioDecoder::poll_frame) until `Ok(None)`,
/// then [`flush`](AudioDecoder::flush) and drain again. Mirrors [`crate::VideoDecoder`]'s
/// shape ([ADR-0001](../adr/0001-decoder-traits.md)); see
/// [ADR-0003](../adr/0003-audio-decoder-trait.md) for this trait's own decision.
///
/// No shared `AudioDecoderConfig` exists yet (unlike [`crate::VideoDecoderConfig`]) — there
/// is no audio `auto`-dispatch to justify one today, so each backend keeps its own config
/// type (see ADR-0003).
pub trait AudioDecoder {
    /// Stream metadata (updated when sample rate / channels become available).
    fn stream_info(&self) -> &StreamInfo;

    /// Submit one compressed packet. May produce zero or more frames (drain via poll).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when the packet is rejected or the session failed.
    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError>;

    /// Pull the next decoded frame, if any.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] on backend failure.
    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError>;

    /// Signal end-of-input; drain remaining frames with [`poll_frame`](Self::poll_frame).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] on backend failure.
    fn flush(&mut self) -> Result<(), DecodeError>;
}
