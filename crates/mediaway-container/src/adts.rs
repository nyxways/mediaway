//! Mediaway-typed ADTS (raw AAC elementary stream) mux + demux over [`adts-core`].
//!
//! ADTS has no container-level header and no timing metadata of its own — one
//! AAC frame always covers 1024 samples (the standard case this crate's
//! `adts-core` core assumes; see that crate's docs), so [`Demuxer::poll_packet`]
//! synthesizes `pts`/`duration` from a running sample count and the track's
//! sample rate rather than reading them from the bitstream (there is nothing
//! to read — documented here, not silently guessed).

#![forbid(unsafe_code)]

use crate::{Demux, Mux};
use adts_core::{
    AacProfile as CoreAacProfile, AdtsConfig, Demuxer as CoreDemuxer, Muxer as CoreMuxer,
};
use mediaway_common::{CodecKind, Packet, Rational, StreamInfo};

/// Standard AAC-LC frame size in samples — fixed by the format, not signaled
/// in ADTS itself.
const SAMPLES_PER_FRAME: u64 = 1024;

/// ADTS mux error (same as [`adts_core::Error`]).
pub type Error = adts_core::Error;

/// Live ADTS mux session. Fixed `AdtsConfig` for the session's lifetime — ADTS
/// carries no track-registration step (unlike `mp4`/`webm`, it is a single
/// implicit stream).
#[derive(Debug)]
pub struct Muxer {
    inner: CoreMuxer,
    output: Vec<u8>,
}

impl Muxer {
    /// Open a mux session for `sample_rate` (must be a standard ADTS rate) /
    /// `channels`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedSampleRate`] for a non-standard rate.
    pub fn new(sample_rate: u32, channels: u8) -> Result<Self, Error> {
        let inner = CoreMuxer::new(AdtsConfig {
            profile: CoreAacProfile::Lc,
            sample_rate,
            channels,
        })?;
        Ok(Self {
            inner,
            output: Vec::new(),
        })
    }

    /// Append one AAC frame (raw, ADTS header added).
    ///
    /// # Errors
    ///
    /// Returns [`Error::FrameTooLarge`] if `packet.payload` is too large for
    /// ADTS's 13-bit frame-length field.
    pub fn push_packet(&mut self, packet: &Packet) -> Result<(), Error> {
        self.inner.write_frame(&packet.payload, &mut self.output)
    }

    /// No-op — ADTS frames are independently appendable; nothing is buffered
    /// beyond what [`poll_bytes`](Self::poll_bytes) already exposes.
    pub const fn flush(&self) {}

    /// Append available bytes into `out`; returns bytes written.
    pub fn poll_bytes(&mut self, out: &mut Vec<u8>) -> usize {
        let n = self.output.len();
        out.extend_from_slice(&self.output);
        self.output.clear();
        n
    }
}

#[allow(clippy::use_self)]
impl Mux for Muxer {
    type Error = Error;

    fn push_packet(&mut self, packet: &Packet) -> Result<(), Self::Error> {
        Muxer::push_packet(self, packet)
    }

    fn flush(&mut self) {
        Muxer::flush(self);
    }

    fn poll_bytes(&mut self, out: &mut Vec<u8>) -> usize {
        Muxer::poll_bytes(self, out)
    }
}

/// Demuxer wrapping [`adts_core::Demuxer`] with a Mediaway stream cache.
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
    /// parsed (empty before that — ADTS carries no upfront track metadata).
    #[must_use]
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// Next demuxed packet, synthesizing `pts`/`duration` from a running
    /// 1024-samples-per-frame count (see module docs).
    pub fn poll_packet(&mut self) -> Option<Packet> {
        let payload = self.inner.poll_frame().ok().flatten()?;
        let config = self.inner.config()?;
        if self.streams.is_empty() {
            self.streams.push(StreamInfo::Audio {
                id: 0,
                codec: CodecKind::Aac,
                time_base: Rational::new(1, config.sample_rate),
                extra_data: mediaway_common::Bytes::new(),
                sample_rate: config.sample_rate,
                channels: u16::from(config.channels),
            });
        }
        let pts = i64::try_from(self.samples_seen).unwrap_or(i64::MAX);
        self.samples_seen += SAMPLES_PER_FRAME;
        Some(Packet {
            stream_id: 0,
            pts,
            dts: pts,
            duration: SAMPLES_PER_FRAME,
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
#[path = "adts_tests.rs"]
mod tests;
