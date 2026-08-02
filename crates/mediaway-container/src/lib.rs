//! Container facade — shared traits and Mediaway-typed MP4 surface.
//!
//! Pure ISOBMFF lives in [`iso_bmff`]. This crate maps to [`mediaway_common`]
//! types and exposes [`Mux`] / [`Demux`] / [`DemuxDecrypt`].

#![forbid(unsafe_code)]

pub mod adts;
pub mod convert;
pub mod flv;
pub mod mp3;
pub mod mp4;
mod mp4_parser;
pub mod ogg;
pub mod ts;
pub mod wav;
#[cfg(any(feature = "mux", feature = "demux"))]
pub mod webm;

use mediaway_common::{Packet, StreamInfo};

/// Pair a muxer and demuxer for one container format.
pub trait ContainerFormat {
    /// Muxer type (often a typestate root).
    type Muxer;
    /// Demuxer type.
    type Demuxer;

    /// Empty muxer (track registration / open state).
    fn muxer() -> Self::Muxer;
    /// Empty demuxer.
    fn demuxer() -> Self::Demuxer;
}

/// Live mux session: packets in → container bytes out.
pub trait Mux {
    /// Implementation error type.
    type Error;

    /// Push one compressed packet.
    ///
    /// # Errors
    ///
    /// Returns when the packet does not match registered tracks or framing fails.
    fn push_packet(&mut self, packet: &Packet) -> Result<(), Self::Error>;
    /// Flush pending fragments / trailers.
    fn flush(&mut self);
    /// Append available container bytes into `out`; returns bytes written.
    fn poll_bytes(&mut self, out: &mut Vec<u8>) -> usize;
}

/// Demux session: container bytes in → packets out.
pub trait Demux {
    /// Feed container bytes (sans-io; caller owns I/O).
    fn push_bytes(&mut self, chunk: &[u8]);
    /// Tracks discovered so far (e.g. after `moov`).
    fn streams(&self) -> &[StreamInfo];
    /// Next demuxed packet, if any.
    fn poll_packet(&mut self) -> Option<Packet>;
}

/// Optional `ClearKey` hook for demuxers that support ISO CENC.
pub trait DemuxDecrypt: Demux {
    /// Supply a 128-bit content key for sample decrypt.
    fn set_decryption_key(&mut self, key: [u8; 16]);
    /// Drop any previously set key.
    fn clear_decryption_key(&mut self);
}
