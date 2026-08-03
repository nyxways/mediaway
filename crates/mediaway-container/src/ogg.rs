//! Mediaway-typed Ogg (Opus/Vorbis transport) mux + demux over [`ogg-core`].
//!
//! [`ogg-core`] carries the logical-bitstream framing only — it has no notion of
//! which codec's packets it's transporting. [`Demuxer`] identifies the codec
//! by reading the first (identification-header) packet's well-known magic
//! and codec-specific fixed fields (`OpusHead` per RFC 7845 §5.1; the Vorbis
//! identification header per the Vorbis I spec §4.2.2) — this is reading a
//! codec's declared config bytes, the same boundary `iso-bmff` already
//! crosses for AAC's `esds`/`AudioSpecificConfig`, not decoding audio.
//!
//! `granule_position` becomes `Packet::pts` directly: for Opus it is always a
//! 48 kHz sample count (RFC 7845 §4, independent of the stream's nominal
//! input rate); for Vorbis it is a sample count at the stream's own rate
//! (parsed from the identification header). Both cases: `time_base = 1 /
//! sample_rate` makes `pts` unit-correct without further conversion.

#![forbid(unsafe_code)]

use crate::{Demux, Mux};
use mediaway_common::{Bytes, CodecKind, Packet, Rational, StreamInfo};
use ogg_core::{Demuxer as CoreDemuxer, Muxer as CoreMuxer};

/// Ogg mux/demux error (same as [`ogg_core::Error`]).
pub type Error = ogg_core::Error;

/// Live mux session for one logical bitstream (one Ogg `serial`).
#[derive(Debug)]
pub struct Muxer {
    inner: CoreMuxer,
    output: Vec<u8>,
}

impl Muxer {
    /// Open a mux session for logical bitstream `serial`.
    #[must_use]
    pub const fn new(serial: u32) -> Self {
        Self {
            inner: CoreMuxer::new(serial),
            output: Vec::new(),
        }
    }

    /// Write one page containing `packet.payload`. `packet.pts` becomes the
    /// page's `granule_position`; `packet.is_discard` becomes the page's
    /// `eos` flag (the last packet of a logical stream is conventionally
    /// marked discard-after by callers that know the stream is ending).
    ///
    /// # Errors
    ///
    /// Returns [`Error::PacketTooLargeForSinglePage`] when `packet.payload`
    /// exceeds a single Ogg page's capacity (this mux always emits one page
    /// per packet — see `ogg-core` crate docs).
    pub fn push_packet(&mut self, packet: &Packet) -> Result<(), Error> {
        self.inner.push_packet(
            &packet.payload,
            packet.pts,
            packet.is_discard,
            &mut self.output,
        )
    }

    /// No-op — every [`push_packet`](Self::push_packet) call already wrote a
    /// complete, independently valid Ogg page.
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

/// Demuxer wrapping [`ogg_core::Demuxer`] with a Mediaway stream cache.
#[derive(Debug, Default)]
pub struct Demuxer {
    inner: CoreDemuxer,
    streams: Vec<StreamInfo>,
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

    /// The single logical-bitstream's [`StreamInfo`], once the identification
    /// header packet has been seen and its codec recognized (empty
    /// otherwise — see module docs on the VP8/Vorbis-style mapping gap: an
    /// unrecognized codec's packets are still dropped, not surfaced as `Err`,
    /// matching `mediaway-container::webm`'s posture).
    #[must_use]
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// Next demuxed packet. Frames from an unrecognized codec (magic doesn't
    /// match `OpusHead`/Vorbis) are skipped.
    pub fn poll_packet(&mut self) -> Option<Packet> {
        loop {
            let p = self.inner.poll_packet().ok().flatten()?;
            if self.streams.is_empty() {
                let Some(info) = identify(&p.data) else {
                    // Not (yet) an identification-header packet we recognize —
                    // real streams put it first, so treat this as "unsupported
                    // logical bitstream" and stop trying rather than
                    // misinterpreting audio-data packets as headers.
                    continue;
                };
                self.streams.push(info);
                continue; // the identification header itself is not audio data
            }
            let stream = &self.streams[0];
            return Some(Packet {
                stream_id: stream.id(),
                pts: p.granule_position,
                dts: p.granule_position,
                duration: 0, // Ogg carries no per-packet duration; only granule position
                is_keyframe: true,
                is_discard: false,
                payload: p.data,
            });
        }
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

const OPUS_HEAD_MAGIC: &[u8] = b"OpusHead";
const VORBIS_ID_MAGIC: &[u8] = b"\x01vorbis";

/// Recognize an Ogg identification-header packet and extract enough to build
/// a [`StreamInfo`]. `None` for anything else (including real audio-data
/// packets from a codec-less-than-identification-header-first stream).
fn identify(packet: &[u8]) -> Option<StreamInfo> {
    if packet.len() >= 19 && packet.starts_with(OPUS_HEAD_MAGIC) {
        let channels = u16::from(packet[9]);
        // RFC 7845 §4: Opus granule position is always counted at 48 kHz,
        // independent of the "input sample rate" field (bytes 12..16) — using
        // anything else here would make `pts` wrong, not just imprecise.
        return Some(StreamInfo::Audio {
            id: 0,
            codec: CodecKind::Opus,
            time_base: Rational::new(1, 48_000),
            extra_data: Bytes::copy_from_slice(packet),
            sample_rate: 48_000,
            channels,
        });
    }
    if packet.len() >= 30 && packet.starts_with(VORBIS_ID_MAGIC) {
        let channels = u16::from(packet[11]);
        let sample_rate = u32::from_le_bytes([packet[12], packet[13], packet[14], packet[15]]);
        if sample_rate == 0 {
            return None;
        }
        return Some(StreamInfo::Audio {
            id: 0,
            codec: CodecKind::Vorbis,
            time_base: Rational::new(1, sample_rate),
            extra_data: Bytes::copy_from_slice(packet),
            sample_rate,
            channels,
        });
    }
    None
}

#[cfg(test)]
#[path = "ogg_tests.rs"]
mod tests;
