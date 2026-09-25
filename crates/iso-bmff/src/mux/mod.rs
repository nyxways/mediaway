//! Typestate MP4 muxer (fMP4 primary) — session only; boxes live in `isobmff`.

#![forbid(unsafe_code)]

#[cfg(feature = "audio")]
use crate::bitstream::strip_adts;
#[cfg(feature = "video")]
use crate::bitstream::{to_avcc, to_hvcc};
use crate::codec_features::check_codec;
use crate::error::Error;
use crate::isobmff::{write_fragment, write_ftyp, write_moov};
use crate::types::{Bytes, Codec, Sample, Track};
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

/// Where one sample's payload landed in the muxer's output stream — see
/// [`Muxer::with_placements`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Placement {
    /// The sample's [`Sample::stream_id`] (the caller's id, not the ISOBMFF `track_ID`).
    pub track_id: u32,
    /// The sample's decode timestamp, as pushed.
    pub dts: i64,
    /// Absolute byte offset of the payload in the muxer's output: counted from the first byte
    /// [`Muxer::poll_bytes`] ever returned, so it is the file offset when every polled byte is
    /// written sequentially from offset 0.
    pub offset: u64,
    /// Payload length in bytes, **as written** — after Annex-B → length-prefixed conversion
    /// (H.264/HEVC) or ADTS stripping (AAC), so it can differ from the pushed payload's length.
    pub len: u32,
}

#[derive(Debug)]
struct Pending {
    track_id: u32,
    /// The caller's stream id (`track_id` is `stream_id + 1`), for [`Placement`]s.
    stream_id: u32,
    base_dts: u64,
    /// Per-sample decode timestamps (media timescale) — durations are derived
    /// from consecutive `dts` deltas at flush time, see [`Muxer::push_packet`].
    dts: SmallVec<[i64; INLINE_SAMPLES]>,
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
    /// Bytes already drained from the front of `output`: `output[i]` is stream byte
    /// `drained + i`.
    drained: u64,
    /// Whether [`Muxer::with_placements`] is on. When off, `placements` is never touched.
    record_placements: bool,
    /// Placements not yet taken by [`Muxer::poll_placements`]; reused across polls.
    placements: Vec<Placement>,
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
            drained: 0,
            record_placements: false,
            placements: Vec::new(),
            _state: PhantomData,
        }
    }

    /// Also record a [`Placement`] for every sample written: where its payload sits in the
    /// output stream. Take them with [`Muxer::poll_placements`].
    ///
    /// For a caller that writes the output to a file and wants to read sample payloads back
    /// later instead of keeping them in memory (e.g. a replay buffer). Off by default; when off
    /// the muxer does no placement work at all. When on, the cost is one 24-byte push per
    /// sample into a `Vec` the caller drains, and nothing else: the output bytes are identical.
    #[must_use]
    pub const fn with_placements(mut self) -> Self {
        self.record_placements = true;
        self
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
            drained: self.drained,
            record_placements: self.record_placements,
            placements: self.placements,
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

    /// Backfill `track_id`'s `extra_data` if it is still empty — for an encoder backend
    /// whose config record (SPS/PPS-derived `avcC`, etc.) is only known after encoding
    /// at least one frame (e.g. `VideoToolbox`, which determines SPS/PPS internally
    /// rather than deriving them from open-time config), so [`Muxer::add_track`]'s
    /// initial [`Track::extra_data`] is necessarily empty. Callers should call this with
    /// the encoder's now-current `extra_data` right before pushing that first packet —
    /// [`Muxer::push_packet`]'s own in-band Annex-B extraction already covers backends
    /// that emit Annex-B-framed samples with in-band parameter sets, so this only
    /// matters for AVCC-native output (no in-band SPS/PPS to extract from). A no-op
    /// once the moov header is written (too late to matter) or once `extra_data` is
    /// already non-empty (never overwrites a real value).
    pub fn set_track_extra_data(&mut self, track_id: u32, extra_data: Bytes) {
        if self.header_written {
            return;
        }
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id)
            && track.extra_data.is_empty()
        {
            track.extra_data = extra_data;
        }
    }

    /// Push a compressed Sample (H.264 and HEVC Annex-B auto-converted to length-prefixed).
    ///
    /// # Annex-B conversion is not optional for H.264/HEVC
    ///
    /// `avcC`/`hvcC` both declare `lengthSizeMinusOne = 3`, so a sample in those tracks
    /// **is** a sequence of 4-byte-length-prefixed NALs. Handing the caller's bytes through
    /// unchanged produces a file whose sample entry and whose `mdat` disagree, and a decoder
    /// reads the first four bytes of a start code as a length: `00 00 00 01` becomes a
    /// one-byte NAL, and everything after it is garbage. That was HEVC's behaviour until
    /// 2026-09-18 — see `adr/0006-hevc-in-mp4.md`.
    ///
    /// Both codecs also **backfill their configuration record** from the parameter sets in the
    /// first packet, for the encoder backends that never publish one out of band. The backfill
    /// only fires while `extra_data` is still empty, so a real record supplied at
    /// registration always wins.
    ///
    /// Sample durations are computed from consecutive `dts` deltas inside each
    /// fragment (standard muxer convention), so `Sample::duration` is optional:
    /// it is only trusted for the **last** sample of a fragment, and when it is
    /// zero the last sample's duration is estimated from the previous sample's
    /// delta (a lone-sample fragment defaults to one media tick). `dts` must be
    /// monotonically non-decreasing per track; out-of-order `dts` degrades to
    /// a 1-tick duration.
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
            #[cfg(feature = "video")]
            Codec::Hevc => {
                let o = to_hvcc(&sample.payload);
                (o.payload, o.hvcc)
            }
            #[cfg(feature = "audio")]
            Codec::Aac => strip_adts(&sample.payload),
            _ => {
                // clone: passthrough codecs keep caller's Bytes; no in-place transform
                (sample.payload.clone(), None)
            }
        };
        if let Some(e) = extra
            && self.tracks[idx].extra_data.is_empty()
        {
            self.tracks[idx].extra_data = e;
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
            p.dts.push(sample.dts);
            p.durations.push(dur);
            p.sizes.push(size);
            p.flags.push(flags);
            p.ctos.push(cto);
            p.payload.extend_from_slice(&payload);
        } else {
            let mut pending = Pending {
                track_id: isobmff_id,
                stream_id: sample.stream_id,
                base_dts,
                dts: SmallVec::with_capacity(batch),
                durations: SmallVec::with_capacity(batch),
                sizes: SmallVec::with_capacity(batch),
                flags: SmallVec::with_capacity(batch),
                ctos: SmallVec::with_capacity(batch),
                payload: Vec::with_capacity(payload.len().saturating_mul(batch)),
            };
            pending.dts.push(sample.dts);
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
            self.drained += self.output_consumed as u64;
            self.output_consumed = 0;
        }
        available
    }

    /// Move the [`Placement`]s recorded since the last call into `out`, in the order the
    /// samples were written (fragment by fragment; within a fragment, push order). Returns how
    /// many were moved. Always zero unless the muxer was built [`Muxer::with_placements`].
    ///
    /// A placement is recorded when its fragment is written, which is also when its bytes
    /// become available to [`Muxer::poll_bytes`]: poll and write the bytes before reading a
    /// payload back at `offset`.
    pub fn poll_placements(&mut self, out: &mut Vec<Placement>) -> usize {
        let n = self.placements.len();
        out.append(&mut self.placements);
        n
    }

    fn flush_track(&mut self, track_id: u32) {
        let Some(pos) = self.pending.iter().position(|p| p.track_id == track_id) else {
            return;
        };
        let mut pending = self.pending.swap_remove(pos);
        if pending.durations.is_empty() {
            return;
        }
        // Sample durations are the dts deltas between consecutive samples
        // (standard muxer convention). The caller-provided duration is only
        // consulted for the LAST sample of the fragment; when it is zero we
        // estimate it from the previous sample's delta, and a lone-sample
        // fragment defaults to one media tick.
        let n = pending.durations.len();
        for i in 0..n.saturating_sub(1) {
            // Non-monotonic dts (caller out of order) clamps to a 1-tick
            // duration instead of a zero/u32::MAX sample: `saturating_sub`
            // saturates at i64::MIN, so clamp the delta explicitly.
            let delta = pending.dts[i + 1].saturating_sub(pending.dts[i]).max(1);
            pending.durations[i] = u32::try_from(delta).unwrap_or(u32::MAX);
        }
        if pending.durations[n - 1] == 0 {
            pending.durations[n - 1] = if n >= 2 { pending.durations[n - 2] } else { 1 };
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
        if self.record_placements {
            // `write_fragment` ends with the `mdat` payload, so it is the output's tail.
            let mut offset = self.drained + (self.output.len() - pending.payload.len()) as u64;
            for (&dts, &len) in pending.dts.iter().zip(&pending.sizes) {
                self.placements.push(Placement {
                    track_id: pending.stream_id,
                    dts,
                    offset,
                    len,
                });
                offset += u64::from(len);
            }
        }
    }
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod tests;
