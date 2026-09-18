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

    /// Supply a track's codec configuration record after [`MuxOpen::begin`].
    ///
    /// For backends that only know their config record after encoding a frame (e.g.
    /// `VideoToolbox`, which derives SPS/PPS internally), `StreamInfo::extra_data` is
    /// still empty at track-registration time and only becomes real once a packet comes
    /// out. This is the hook that lets a caller correct it.
    ///
    /// **Defaults to ignoring the call**, because most containers commit their track
    /// header at `begin()` and cannot revise it afterwards — [`webm::Muxer`] is exactly
    /// that case, since `CodecPrivate` is written into `Tracks` in the EBML header. A
    /// container that cannot honour this drops late `extra_data` silently rather than
    /// failing, which is a real limitation: on such a container, a late-config backend
    /// produces a file with no codec configuration record. Pair those backends with
    /// [`mp4::Muxer`], which buffers `moov` until the first fragment and therefore can.
    fn set_track_extra_data(&mut self, track_id: u32, extra_data: mediaway_common::Bytes) {
        let _ = (track_id, extra_data);
    }
}

/// Track-registration phase of a muxer: add tracks, then [`begin`](Self::begin) into a
/// live [`Mux`].
///
/// Both [`mp4::Muxer`] and [`webm::Muxer`] are typestate — tracks exist only before
/// `begin()`, so "add a track to a running mux" is unrepresentable rather than a runtime
/// error. This trait names that shared shape so a composition layer can be generic over
/// the container instead of hardcoding one; see `mediaway`'s `EncodeSession`.
///
/// `Live: Mux<Error = Self::Error>` deliberately ties both phases to one error type. A
/// muxer whose registration and streaming phases failed differently would need two error
/// parameters everywhere downstream, and neither container has that shape.
pub trait MuxOpen {
    /// The streaming phase this becomes after [`begin`](Self::begin).
    type Live: Mux<Error = Self::Error>;
    /// Implementation error type, shared with [`Self::Live`].
    type Error;

    /// Lowest track id this container accepts.
    ///
    /// Defaults to `0`, which is what ISOBMFF allows. Matroska/`WebM` reserve `TrackNumber`
    /// `0` and reject it outright, so [`webm::Muxer`] raises this to `1`.
    ///
    /// This is here because track numbering is a **container** rule, not a caller's choice.
    /// Without it, a composition layer that hands the encoder's own default id (`0`) to the
    /// muxer works on MP4 and fails on `WebM` for a reason that has nothing to do with the
    /// caller's code — which is exactly what happened the first time `mediaway`'s
    /// `EncodeSession` was pointed at a `WebM` muxer.
    const FIRST_TRACK_ID: u32 = 0;

    /// Register one track, returning its assigned track id.
    ///
    /// # Errors
    ///
    /// Returns when the track is a duplicate, or when the container has no mapping for
    /// the stream's codec.
    fn add_track(&mut self, track: StreamInfo) -> Result<u32, Self::Error>;

    /// Finish registration and begin streaming. Consumes `self`: no track can be added
    /// afterwards.
    fn begin(self) -> Self::Live;
}

/// Any container's mux failure, as one type.
///
/// Exists so a layer generic over [`MuxOpen`] can report failures without a generic error
/// parameter of its own — `mediaway`'s `PipelineError::Mux` wraps this, so adding a
/// container does not change `PipelineError`'s shape.
///
/// **Closed by design.** A third-party [`MuxOpen`] implementation cannot convert into this,
/// so it cannot be used with a layer that requires `Error: Into<ContainerError>`. The
/// alternative — `Box<dyn Error>` — would make every mux failure untypeable by the caller,
/// and this crate owns every container mediaway ships. `#[non_exhaustive]` keeps adding one
/// non-breaking.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ContainerError {
    /// MP4 / fragmented-MP4 mux failure.
    #[error("MP4 mux error: {0}")]
    Mp4(#[from] mp4::Error),
    /// `WebM` / Matroska mux failure.
    #[cfg(feature = "mux")]
    #[error("WebM mux error: {0}")]
    Webm(#[from] webm::Error),
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
