//! Mediaway-typed MPEG audio (Layer III / "MP3") mux + demux over [`mpeg_audio`].
//!
//! Like ADTS, MPEG audio carries no timing metadata of its own —
//! [`Demuxer::poll_packet`] synthesizes `pts`/`duration` from a running sample
//! count using the standard per-version frame size (1152 samples for MPEG-1
//! Layer III, 576 for MPEG-2/2.5 Layer III — half-rate).
//!
//! [`Muxer`] does not implement [`crate::Mux`]: real Layer III streams flip a
//! per-frame padding bit to average out fractional frame lengths (a
//! bit-reservoir detail `mpeg_audio::Muxer::write_frame` requires as an
//! explicit argument), which the generic `Packet`-based trait has no slot
//! for — exposing the real, correct API is more honest than forcing a fit
//! that would silently default padding away.

#![forbid(unsafe_code)]

use crate::Demux;
use mediaway_common::{Bytes, CodecKind, Packet, Rational, StreamInfo};
use mpeg_audio::{Demuxer as CoreDemuxer, FrameHeader, MpegVersion, Muxer as CoreMuxer};

/// MP3 mux/demux error (same as [`mpeg_audio::Error`]).
pub type Error = mpeg_audio::Error;

/// Samples per Layer III frame — 1152 for MPEG-1, 576 (half-rate) for MPEG-2/2.5.
#[must_use]
pub const fn samples_per_frame(version: MpegVersion) -> u64 {
    match version {
        MpegVersion::Mpeg1 => 1152,
        // Covers MPEG-2/2.5 (half-rate) and any future `MpegVersion` variant.
        _ => 576,
    }
}

/// Live mux session for a fixed [`FrameHeader`].
///
/// Bitrate/sample-rate/channel mode stay constant for the session's
/// lifetime, matching real Layer III streams this crate targets — VBR would
/// need a new header per frame, out of scope.
#[derive(Debug)]
pub struct Muxer {
    inner: CoreMuxer,
}

impl Muxer {
    /// Open a mux session for `header`.
    ///
    /// # Errors
    ///
    /// Returns an error for a non-standard bitrate/sample-rate combination.
    pub fn new(header: FrameHeader) -> Result<Self, Error> {
        Ok(Self {
            inner: CoreMuxer::new(header)?,
        })
    }

    /// Append one already-encoded Layer III frame body.
    ///
    /// # Errors
    ///
    /// Returns an error when `frame_body`'s length doesn't match what the
    /// header's bitrate/sample-rate/`padding` combination requires.
    pub fn write_frame(
        &self,
        frame_body: &[u8],
        padding: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.inner.write_frame(frame_body, padding, out)
    }
}

/// Demuxer wrapping [`mpeg_audio::Demuxer`] with a Mediaway stream cache.
#[derive(Debug, Default)]
pub struct Demuxer {
    inner: CoreDemuxer,
    streams: Vec<StreamInfo>,
    samples_seen: u64,
}

impl Demuxer {
    /// Empty demuxer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes.
    pub fn push_bytes(&mut self, chunk: &[u8]) {
        self.inner.push_bytes(chunk);
    }

    /// The single implicit stream, once the first frame's header has been
    /// parsed (empty before that).
    #[must_use]
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// Next demuxed packet, synthesizing `pts`/`duration` from a running
    /// samples-per-frame count (see module docs).
    pub fn poll_packet(&mut self) -> Option<Packet> {
        let payload = self.inner.poll_frame().ok().flatten()?;
        let header = self.inner.header()?;
        if self.streams.is_empty() {
            let channels = if matches!(header.channel_mode, mpeg_audio::ChannelMode::Mono) {
                1
            } else {
                2
            };
            self.streams.push(StreamInfo::Audio {
                id: 0,
                codec: CodecKind::Mp3,
                time_base: Rational::new(1, header.sample_rate),
                extra_data: Bytes::new(),
                sample_rate: header.sample_rate,
                channels,
            });
        }
        let frame_samples = samples_per_frame(header.version);
        let pts = i64::try_from(self.samples_seen).unwrap_or(i64::MAX);
        self.samples_seen += frame_samples;
        Some(Packet {
            stream_id: 0,
            pts,
            dts: pts,
            duration: frame_samples,
            is_keyframe: true,
            is_discard: false,
            payload,
        })
    }
}

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

#[cfg(test)]
#[path = "mp3_tests.rs"]
mod tests;
