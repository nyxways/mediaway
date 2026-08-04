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
//! `granule_position` becomes `Packet::pts` after per-packet refinement: for
//! Opus the facade back-computes each packet's own end position from its
//! page's granule using RFC 6716 TOC frame durations (packets completed on
//! one page all carry the page granule in `ogg-core` — see its ADR-0001), so
//! `pts` is the packet's end position in 48 kHz samples and `duration` its
//! TOC-derived length; for Vorbis `pts` remains the page granule (per-packet
//! block-size parsing is out of scope). Both cases: `time_base = 1 /
//! sample_rate` makes `pts` unit-correct without further conversion.

#![forbid(unsafe_code)]

use crate::{Demux, Mux};
use mediaway_common::{Bytes, CodecKind, Packet, Rational, StreamInfo};
use ogg_core::Packet as CorePacket;
use ogg_core::{Demuxer as CoreDemuxer, Muxer as CoreMuxer};
use std::collections::VecDeque;

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
    /// Completed packets of the current Opus page (completion order, with
    /// TOC-derived durations) — buffered until the page is complete so each
    /// packet's end position can be back-computed from the page granule.
    page: Vec<(CorePacket, i64)>,
    /// Packets ready to emit after back-computation.
    ready: VecDeque<Packet>,
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
            if let Some(p) = self.ready.pop_front() {
                return Some(p);
            }
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
            if !matches!(self.streams[0].codec(), CodecKind::Opus) {
                // Vorbis et al.: no per-packet duration parsing (out of scope),
                // so the page-level granule is the best available position.
                return Some(Packet {
                    stream_id: self.streams[0].id(),
                    pts: p.granule_position,
                    dts: p.granule_position,
                    duration: 0, // Ogg carries no per-packet duration; only granule position
                    is_keyframe: true,
                    is_discard: false,
                    payload: p.data,
                });
            }
            // Opus: packets completed on one page all carry the page granule
            // (the position after the *last* of them). Back-compute each
            // packet's own end position from the page granule minus the TOC
            // frame durations of the packets completed after it on the page.
            let duration = opus_packet_duration(&p.data);
            if !self.page.is_empty()
                && (p.granule_position != self.page[0].0.granule_position
                    || p.page_count != self.page[0].0.page_count)
            {
                self.finalize_page();
            }
            self.page.push((p, duration));
            let page_len = u32::try_from(self.page.len()).unwrap_or(u32::MAX);
            if page_len == self.page[0].0.page_count {
                self.finalize_page();
            }
        }
    }

    /// Emit the buffered Opus page in completion order with back-computed pts.
    fn finalize_page(&mut self) {
        let stream_id = self.streams[0].id();
        let granule = self.page[0].0.granule_position;
        let mut suffix = 0i64;
        for (p, duration) in self.page.iter().rev() {
            let pts = granule - suffix;
            self.ready.push_front(Packet {
                stream_id,
                pts,
                dts: pts,
                duration: u64::try_from(*duration).unwrap_or(0),
                is_keyframe: true,
                is_discard: false,
                // clone: CorePacket outlives the emitted Packet (page buffer
                // drains after); Bytes is a refcounted handle, not a copy.
                payload: p.data.clone(),
            });
            suffix += *duration;
        }
        self.page.clear();
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

/// Opus packet duration in 48 kHz samples (RFC 6716 §3.1/§3.2): the TOC
/// byte's `config` picks a per-frame size (SILK 10-60 ms, hybrid 10-20 ms,
/// CELT 2.5-20 ms — Table 2) and its `c` code the frame count; zero-length
/// (DTX) frames contribute no samples. Validated against real encoder output:
/// per-page TOC sums equal the page granule deltas exactly.
fn opus_packet_duration(packet: &[u8]) -> i64 {
    let Some(&toc) = packet.first() else {
        return 0;
    };
    let config = (toc >> 3) & 0x1F;
    let frame: i64 = match config {
        0..=11 => i64::from(480 * [1, 2, 4, 6][usize::from(config & 3)]),
        12..=15 => i64::from(480 << (config & 1)),
        _ => i64::from(120 << (config & 3)), // 16..=31 CELT
    };
    let frames = match toc & 0x3 {
        0 => 1,
        1 => 2,
        2 => {
            // Two frames, first length-coded (§3.2.4); a 0-length (DTX)
            // first frame contributes no samples.
            let Some(&l) = packet.get(1) else {
                return 0;
            };
            if l == 0 { 1 } else { 2 }
        }
        _ => {
            // Code 3: second byte = v(bit 0) p(bit 1) M(bits 2-7); M is the
            // frame count (MUST NOT be zero, RFC 6716 §3.2.5).
            let Some(&b1) = packet.get(1) else {
                return 0;
            };
            let m = i64::from((b1 >> 2) & 0x3F).max(1);
            if b1 & 1 == 1 {
                // VBR: walk the length-coded sizes of the first M-1 frames
                // (§3.2.1); 0-length (DTX) frames contribute no samples. The
                // last frame's length is inferred, assume it is present.
                let mut pos = 2usize;
                let mut present = 0i64;
                for _ in 0..m - 1 {
                    let Some(&l) = packet.get(pos) else {
                        break;
                    };
                    pos += 1;
                    let len = if l >= 252 {
                        let Some(&l2) = packet.get(pos) else {
                            break;
                        };
                        pos += 1;
                        i64::from(l2) * 4 + i64::from(l)
                    } else {
                        i64::from(l)
                    };
                    if len > 0 {
                        present += 1;
                    }
                }
                present + 1
            } else {
                m
            }
        }
    };
    frames * frame
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
