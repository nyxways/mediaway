//! Mediaway-typed MPEG-2 Transport Stream mux + demux over [`mpeg_ts`].
//!
//! MPEG-TS's PTS/DTS are always a 90 kHz clock (ISO/IEC 13818-1) — a
//! per-track [`mediaway_common::Rational`] time base like other facades use
//! would be a fiction here, not a convenience. [`Demuxer`] uses a fixed
//! `1 / 90_000` `time_base` for every [`StreamInfo`] it produces, so
//! `Packet::pts`/`dts` need no conversion. [`Muxer`] does **not** implement
//! [`crate::Mux`] for the same reason `mp3`/`wav` don't: real callers of a
//! 90 kHz-native mux need to pass `pts_90k`/`dts_90k` explicitly, not a
//! `Packet`'s arbitrary-time-base `pts` silently reinterpreted as 90 kHz.

#![forbid(unsafe_code)]

use crate::Demux;
use mediaway_common::{Bytes, CodecKind, Packet, Rational, StreamInfo};
use mpeg_ts::{Demuxer as CoreDemuxer, Muxer as CoreMuxer};
pub use mpeg_ts::{ElementaryStream, StreamType};

/// MPEG-TS mux/demux error (same as [`mpeg_ts::Error`]).
pub type Error = mpeg_ts::Error;

/// 90 kHz — the fixed MPEG-TS system clock (not a per-track choice).
pub const TS_TIME_BASE: Rational = Rational::new(1, 90_000);

/// Live mux session for one single program's elementary streams (identified
/// by TS PID — see [`ElementaryStream`]).
#[derive(Debug)]
pub struct Muxer {
    inner: CoreMuxer,
}

impl Muxer {
    /// Start a mux session. `pmt_pid` and every stream's `pid` must be in
    /// `2..=0x1FFF` (`0`/`1` are reserved for PAT/CAT).
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidPid`] for a reserved or out-of-range PID.
    pub fn new(
        program_number: u16,
        pmt_pid: u16,
        streams: &[ElementaryStream],
    ) -> Result<Self, Error> {
        Ok(Self {
            inner: CoreMuxer::new(program_number, pmt_pid, streams)?,
        })
    }

    /// Write PAT + PMT packets. Call once at the start and periodically
    /// thereafter — real players expect PAT/PMT to repeat.
    pub fn write_pat_pmt(&mut self, out: &mut Vec<u8>) {
        self.inner.write_pat_pmt(out);
    }

    /// Packetize one access unit for `pid` into PES + TS packets.
    /// `pts_90k`/`dts_90k` are the real MPEG-TS 90 kHz clock values — not a
    /// track's own `time_base` units (see module docs).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownPid`] when `pid` wasn't registered in [`new`](Self::new).
    pub fn write_access_unit(
        &mut self,
        pid: u16,
        data: &[u8],
        pts_90k: u64,
        dts_90k: Option<u64>,
        random_access: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.inner
            .write_access_unit(pid, data, pts_90k, dts_90k, random_access, out)
    }
}

/// Demuxer wrapping [`mpeg_ts::Demuxer`] with a Mediaway stream cache.
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

    /// Feed bytes (need not be 188-byte aligned across calls).
    ///
    /// Only appends to the internal buffer — the core demuxer parses PAT/PMT
    /// lazily as TS packets are actually consumed inside
    /// [`poll_packet`](Self::poll_packet)/[`finish`](Self::finish), so
    /// [`streams`](Self::streams) does not update until one of those runs.
    pub fn push_bytes(&mut self, chunk: &[u8]) {
        self.inner.push_bytes(chunk);
    }

    /// Streams whose `stream_type` maps to a [`CodecKind`] (H.264/HEVC/AAC/MP3
    /// — everything `StreamType` currently has); `id` is the TS PID. Empty
    /// until [`poll_packet`](Self::poll_packet) has actually consumed the PMT
    /// packet (see that method's docs on this crate's lazy PSI parsing).
    #[must_use]
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// Next demuxed packet.
    pub fn poll_packet(&mut self) -> Option<Packet> {
        loop {
            let Some(unit) = self.inner.poll_access_unit().ok().flatten() else {
                // Nothing ready yet, but the attempt may still have consumed
                // PAT/PMT packets from the buffer — refresh before returning
                // so `streams()` reflects it even on a `None` poll.
                self.sync_streams();
                return None;
            };
            self.sync_streams();
            if !self.streams.iter().any(|s| s.id() == u32::from(unit.pid)) {
                continue; // PID with no CodecKind mapping — see `streams` docs
            }
            let pts = i64::try_from(unit.pts_90k).unwrap_or(i64::MAX);
            let dts = unit
                .dts_90k
                .map_or(pts, |d| i64::try_from(d).unwrap_or(i64::MAX));
            return Some(Packet {
                stream_id: u32::from(unit.pid),
                pts,
                dts,
                duration: 0, // MPEG-TS signals no explicit per-access-unit duration
                is_keyframe: unit.random_access,
                is_discard: false,
                payload: unit.data,
            });
        }
    }

    /// Force-emit whatever is still accumulating per PID — call once at the
    /// end of a stream so the very last access unit per PID isn't lost (PES
    /// boundaries are only confirmed once the *next* packet on the same PID
    /// starts).
    pub fn finish(&mut self) -> Vec<Packet> {
        self.sync_streams();
        self.inner
            .finish()
            .into_iter()
            .filter(|u| self.streams.iter().any(|s| s.id() == u32::from(u.pid)))
            .map(|unit| {
                let pts = i64::try_from(unit.pts_90k).unwrap_or(i64::MAX);
                let dts = unit
                    .dts_90k
                    .map_or(pts, |d| i64::try_from(d).unwrap_or(i64::MAX));
                Packet {
                    stream_id: u32::from(unit.pid),
                    pts,
                    dts,
                    duration: 0,
                    is_keyframe: unit.random_access,
                    is_discard: false,
                    payload: unit.data,
                }
            })
            .collect()
    }

    fn sync_streams(&mut self) {
        let inner = self.inner.streams();
        if inner.len() == self.streams.len() {
            return;
        }
        self.streams = inner.iter().copied().filter_map(to_stream_info).collect();
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

fn to_stream_info(s: ElementaryStream) -> Option<StreamInfo> {
    let id = u32::from(s.pid);
    match s.stream_type {
        StreamType::H264 => Some(StreamInfo::Video {
            id,
            codec: CodecKind::H264,
            time_base: TS_TIME_BASE,
            // MPEG-TS carries no explicit width/height; real dimensions come
            // from the SPS inside the elementary stream, not parsed here.
            geometry: mediaway_common::VideoGeometry {
                width: 0,
                height: 0,
            },
            extra_data: Bytes::new(),
        }),
        StreamType::Hevc => Some(StreamInfo::Video {
            id,
            codec: CodecKind::Hevc,
            time_base: TS_TIME_BASE,
            geometry: mediaway_common::VideoGeometry {
                width: 0,
                height: 0,
            },
            extra_data: Bytes::new(),
        }),
        StreamType::Aac => Some(StreamInfo::Audio {
            id,
            codec: CodecKind::Aac,
            time_base: TS_TIME_BASE,
            extra_data: Bytes::new(),
            sample_rate: 0,
            channels: 0,
        }),
        StreamType::Mp3 => Some(StreamInfo::Audio {
            id,
            codec: CodecKind::Mp3,
            time_base: TS_TIME_BASE,
            extra_data: Bytes::new(),
            sample_rate: 0,
            channels: 0,
        }),
        // Any future `StreamType` variant — no `CodecKind` mapping yet.
        _ => None,
    }
}

#[cfg(test)]
#[path = "ts_tests.rs"]
mod tests;
