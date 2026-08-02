//! Mediaway-typed MP4 surface over [`iso_bmff`].

#![forbid(unsafe_code)]

use crate::convert::{from_packet, from_stream_info, to_packet, to_stream_info};
use crate::{ContainerFormat, Demux, DemuxDecrypt, Mux};
#[cfg(feature = "demux")]
use iso_bmff::Demuxer as IsoDemuxer;
#[cfg(feature = "mux")]
use iso_bmff::Muxer as IsoMuxer;
#[cfg(feature = "mux")]
use iso_bmff::mux::{Live as IsoLive, Open as IsoOpen};
use mediaway_common::{Packet, StreamInfo};

/// Default fragment batch size.
#[cfg(feature = "mux")]
pub use iso_bmff::DEFAULT_FRAGMENT_BATCH;
/// Re-export freestanding ISOBMFF helpers (box walk, etc.).
pub use iso_bmff::isobmff;
/// MP4 error (same as [`iso_bmff::Error`]).
pub type Error = iso_bmff::Error;

/// Track registration phase.
#[derive(Debug, Clone, Copy, Default)]
pub struct Open;

/// Streaming phase.
#[derive(Debug, Clone, Copy, Default)]
pub struct Live;

/// Format marker.
#[derive(Debug, Clone, Copy, Default)]
pub struct Mp4;

/// Open-state muxer.
#[cfg(feature = "mux")]
#[derive(Debug)]
pub struct Muxer<S = Open> {
    open: Option<IsoMuxer<IsoOpen>>,
    live: Option<IsoMuxer<IsoLive>>,
    tracks_cache: Vec<StreamInfo>,
    _state: core::marker::PhantomData<S>,
}

#[cfg(feature = "mux")]
impl Muxer<Open> {
    /// Empty muxer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: Some(IsoMuxer::new()),
            live: None,
            tracks_cache: Vec::new(),
            _state: core::marker::PhantomData,
        }
    }

    /// Custom fragment batch.
    #[must_use]
    pub fn with_fragment_batch(batch: usize) -> Self {
        Self {
            open: Some(IsoMuxer::with_fragment_batch(batch)),
            live: None,
            tracks_cache: Vec::new(),
            _state: core::marker::PhantomData,
        }
    }

    /// Register a track.
    ///
    /// # Errors
    ///
    /// Invalid or duplicate track id.
    pub fn add_track(&mut self, track: StreamInfo) -> Result<u32, Error> {
        let open = self.open.as_mut().ok_or(Error::InvalidTrack)?;
        let id = open.add_track(from_stream_info(&track))?;
        self.tracks_cache.push(track);
        Ok(id)
    }

    /// Begin streaming.
    #[must_use]
    pub fn begin(mut self) -> Muxer<Live> {
        let open = self.open.take().unwrap_or_default();
        Muxer {
            open: None,
            live: Some(open.begin()),
            tracks_cache: self.tracks_cache,
            _state: core::marker::PhantomData,
        }
    }

    /// Registered tracks.
    #[must_use]
    pub fn tracks(&self) -> &[StreamInfo] {
        &self.tracks_cache
    }
}

#[cfg(feature = "mux")]
impl Default for Muxer<Open> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "mux")]
impl Muxer<Live> {
    /// Push one packet.
    ///
    /// # Errors
    ///
    /// Invalid packet for registered tracks.
    pub fn push_packet(&mut self, packet: &Packet) -> Result<(), Error> {
        let live = self.live.as_mut().ok_or(Error::InvalidPacket)?;
        live.push_packet(&from_packet(packet))
    }

    /// Flush fragments.
    pub fn flush(&mut self) {
        if let Some(live) = self.live.as_mut() {
            live.flush();
        }
    }

    /// Poll container bytes.
    pub fn poll_bytes(&mut self, out: &mut Vec<u8>) -> usize {
        self.live.as_mut().map_or(0, |m| m.poll_bytes(out))
    }

    /// Registered tracks.
    #[must_use]
    pub fn tracks(&self) -> &[StreamInfo] {
        &self.tracks_cache
    }
}

/// Demuxer wrapping [`iso_bmff::Demuxer`] with Mediaway stream cache.
#[cfg(feature = "demux")]
#[derive(Debug, Default)]
pub struct Demuxer {
    inner: IsoDemuxer,
    streams: Vec<StreamInfo>,
}

#[cfg(feature = "demux")]
impl Demuxer {
    /// Empty demuxer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes.
    pub fn push_bytes(&mut self, chunk: &[u8]) {
        self.inner.push_bytes(chunk);
        self.sync_streams();
    }

    /// Tracks.
    #[must_use]
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// Next packet.
    pub fn poll_packet(&mut self) -> Option<Packet> {
        self.inner.poll_packet().map(to_packet)
    }

    /// `ClearKey` content key.
    pub const fn set_decryption_key(&mut self, key: [u8; 16]) {
        self.inner.set_decryption_key(key);
    }

    /// Clear decryption key.
    pub const fn clear_decryption_key(&mut self) {
        self.inner.clear_decryption_key();
    }

    fn sync_streams(&mut self) {
        let inner = self.inner.streams();
        if inner.len() != self.streams.len() {
            self.streams = inner.iter().map(to_stream_info).collect();
        }
    }
}

#[cfg(all(feature = "mux", feature = "demux"))]
impl ContainerFormat for Mp4 {
    type Muxer = Muxer<Open>;
    type Demuxer = Demuxer;

    fn muxer() -> Self::Muxer {
        Muxer::new()
    }

    fn demuxer() -> Self::Demuxer {
        Demuxer::new()
    }
}

#[cfg(feature = "mux")]
#[allow(clippy::use_self)]
impl Mux for Muxer<Live> {
    type Error = Error;

    fn push_packet(&mut self, packet: &Packet) -> Result<(), Self::Error> {
        Muxer::<Live>::push_packet(self, packet)
    }

    fn flush(&mut self) {
        Muxer::<Live>::flush(self);
    }

    fn poll_bytes(&mut self, out: &mut Vec<u8>) -> usize {
        Muxer::<Live>::poll_bytes(self, out)
    }
}

#[cfg(feature = "demux")]
#[allow(clippy::use_self)]
impl Demux for Demuxer {
    fn push_bytes(&mut self, chunk: &[u8]) {
        Demuxer::push_bytes(self, chunk);
    }

    fn streams(&self) -> &[StreamInfo] {
        Demuxer::streams(self)
    }

    fn poll_packet(&mut self) -> Option<Packet> {
        Demuxer::poll_packet(self)
    }
}

#[cfg(feature = "demux")]
#[allow(clippy::use_self)]
impl DemuxDecrypt for Demuxer {
    fn set_decryption_key(&mut self, key: [u8; 16]) {
        Demuxer::set_decryption_key(self, key);
    }

    fn clear_decryption_key(&mut self) {
        Demuxer::clear_decryption_key(self);
    }
}

/// Module path for typestate markers (`mp4::mux::Muxer`).
#[cfg(feature = "mux")]
pub mod mux {
    pub use super::{Live, Muxer, Open};
}

/// Box-tree helpers for probe/debug CLIs.
pub mod mp4_parser {
    pub use crate::mp4_parser::*;
}
