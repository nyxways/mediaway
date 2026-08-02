//! Typestate MP4 muxer (fMP4 primary) — session only; boxes live in `isobmff`.

#![forbid(unsafe_code)]

#[cfg(feature = "audio")]
use crate::bitstream::strip_adts;
#[cfg(feature = "video")]
use crate::bitstream::to_avcc;
use crate::codec_features::check_codec;
use crate::error::Error;
use crate::isobmff::{write_fragment, write_ftyp, write_moov};
use crate::types::{Codec, Sample, Track};
use crate::{INLINE_SAMPLES, INLINE_TRACKS};
use smallvec::SmallVec;
use std::marker::PhantomData;

/// Track registration phase — call [`Muxer::begin`] before packets.
#[derive(Debug, Clone, Copy, Default)]
pub struct Open;

/// Streaming phase — packet/byte API.
#[derive(Debug, Clone, Copy, Default)]
pub struct Live;

/// Default samples per fragment.
pub const DEFAULT_FRAGMENT_BATCH: usize = 30;

const _: () = assert!(
    DEFAULT_FRAGMENT_BATCH <= INLINE_SAMPLES,
    "INLINE_SAMPLES must cover the default fragment batch"
);

#[derive(Debug)]
struct Pending {
    track_id: u32,
    base_dts: u64,
    durations: SmallVec<[u32; INLINE_SAMPLES]>,
    sizes: SmallVec<[u32; INLINE_SAMPLES]>,
    flags: SmallVec<[u32; INLINE_SAMPLES]>,
    ctos: SmallVec<[i32; INLINE_SAMPLES]>,
    payload: Vec<u8>,
}

/// Sans-IO fragmented MP4 muxer.
#[derive(Debug)]
pub struct Muxer<S = Open> {
    tracks: SmallVec<[Track; INLINE_TRACKS]>,
    output: Vec<u8>,
    output_consumed: usize,
    header_written: bool,
    sequence: u32,
    batch: usize,
    pending: SmallVec<[Pending; INLINE_TRACKS]>,
    _state: PhantomData<S>,
}

impl Muxer<Open> {
    /// Empty muxer in track-registration state.
    #[must_use]
    pub fn new() -> Self {
        Self::with_fragment_batch(DEFAULT_FRAGMENT_BATCH)
    }

    /// Muxer with custom fragment batch size.
    #[must_use]
    pub fn with_fragment_batch(batch: usize) -> Self {
        Self {
            tracks: SmallVec::new(),
            output: Vec::with_capacity(64 * 1024),
            output_consumed: 0,
            header_written: false,
            sequence: 0,
            batch: batch.max(1),
            pending: SmallVec::new(),
            _state: PhantomData,
        }
    }

    /// Registered tracks so far.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Register a track. `id` must be unique.
    pub fn add_track(&mut self, track: Track) -> Result<u32, Error> {
        check_codec(track.codec)?;
        if self.tracks.iter().any(|t| t.id == track.id) {
            return Err(Error::InvalidTrack);
        }
        let id = track.id;
        self.tracks.push(track);
        Ok(id)
    }

    /// Lock tracks and enter the live streaming state.
    #[must_use]
    pub fn begin(self) -> Muxer<Live> {
        Muxer {
            tracks: self.tracks,
            output: self.output,
            output_consumed: self.output_consumed,
            header_written: self.header_written,
            sequence: self.sequence,
            batch: self.batch,
            pending: self.pending,
            _state: PhantomData,
        }
    }
}

impl Default for Muxer<Open> {
    fn default() -> Self {
        Self::new()
    }
}

impl Muxer<Live> {
    /// Registered tracks (may gain extradata from Annex-B / ADTS).
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Push a compressed Sample (H.264 Annex-B auto-converted to AVCC).
    pub fn push_packet(&mut self, sample: &Sample) -> Result<(), Error> {
        let idx = self
            .tracks
            .iter()
            .position(|t| t.id == sample.stream_id)
            .ok_or(Error::InvalidPacket)?;

        let track_codec = self.tracks[idx].codec;
        check_codec(track_codec)?;

        let (payload, extra) = match track_codec {
            #[cfg(feature = "video")]
            Codec::H264 => {
                let o = to_avcc(&sample.payload);
                (o.payload, o.avcc)
            }
            #[cfg(feature = "audio")]
            Codec::Aac => strip_adts(&sample.payload),
            _ => {
                // clone: passthrough codecs keep caller's Bytes; no in-place transform
                (sample.payload.clone(), None)
            }
        };
        if let Some(e) = extra {
            if self.tracks[idx].extra_data.is_empty() {
                self.tracks[idx].extra_data = e;
            }
        }

        if !self.header_written {
            write_ftyp(&mut self.output, &self.tracks);
            write_moov(&mut self.output, &self.tracks);
            self.header_written = true;
        }

        let isobmff_id = sample.stream_id.saturating_add(1);
        let size = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        let dur = u32::try_from(sample.duration.min(u64::from(u32::MAX))).unwrap_or(u32::MAX);
        let flags = if sample.is_keyframe {
            0x0200_0000
        } else {
            0x0101_0000
        };
        let cto = i32::try_from(
            sample
                .pts
                .saturating_sub(sample.dts)
                .clamp(i64::from(i32::MIN), i64::from(i32::MAX)),
        )
        .unwrap_or(0);

        let batch = self.batch;
        let key_flush = sample.is_keyframe
            && self.tracks[idx].codec != Codec::Aac
            && self
                .pending
                .iter()
                .any(|p| p.track_id == isobmff_id && !p.durations.is_empty());
        if key_flush
            || self
                .pending
                .iter()
                .any(|p| p.track_id == isobmff_id && p.durations.len() >= batch)
        {
            self.flush_track(isobmff_id);
        }

        let base_dts = u64::try_from(sample.dts.max(0)).unwrap_or(0);
        if let Some(p) = self.pending.iter_mut().find(|p| p.track_id == isobmff_id) {
            p.durations.push(dur);
            p.sizes.push(size);
            p.flags.push(flags);
            p.ctos.push(cto);
            p.payload.extend_from_slice(&payload);
        } else {
            let mut pending = Pending {
                track_id: isobmff_id,
                base_dts,
                durations: SmallVec::with_capacity(batch),
                sizes: SmallVec::with_capacity(batch),
                flags: SmallVec::with_capacity(batch),
                ctos: SmallVec::with_capacity(batch),
                payload: Vec::with_capacity(payload.len().saturating_mul(batch)),
            };
            pending.durations.push(dur);
            pending.sizes.push(size);
            pending.flags.push(flags);
            pending.ctos.push(cto);
            pending.payload.extend_from_slice(&payload);
            self.pending.push(pending);
        }

        if self
            .pending
            .iter()
            .any(|p| p.track_id == isobmff_id && p.durations.len() >= batch)
        {
            self.flush_track(isobmff_id);
        }
        Ok(())
    }

    /// Flush all pending fragments.
    pub fn flush(&mut self) {
        let mut ids: SmallVec<[u32; INLINE_TRACKS]> =
            self.pending.iter().map(|p| p.track_id).collect();
        ids.sort_unstable();
        for id in ids {
            self.flush_track(id);
        }
    }

    /// Append available output bytes into `out`.
    pub fn poll_bytes(&mut self, out: &mut Vec<u8>) -> usize {
        let available = self.output.len().saturating_sub(self.output_consumed);
        if available == 0 {
            return 0;
        }
        out.extend_from_slice(&self.output[self.output_consumed..]);
        self.output_consumed = self.output.len();
        if self.output_consumed >= 64 * 1024 {
            self.output.drain(..self.output_consumed);
            self.output_consumed = 0;
        }
        available
    }

    fn flush_track(&mut self, track_id: u32) {
        let Some(pos) = self.pending.iter().position(|p| p.track_id == track_id) else {
            return;
        };
        let pending = self.pending.swap_remove(pos);
        if pending.durations.is_empty() {
            return;
        }
        self.sequence = self.sequence.saturating_add(1);
        write_fragment(
            &mut self.output,
            self.sequence,
            pending.track_id,
            pending.base_dts,
            &pending.durations,
            &pending.sizes,
            &pending.flags,
            &pending.ctos,
            &pending.payload,
        );
    }
}
