//! Sans-IO `WebM`/Matroska muxer: `add_track` → `begin` → `push_frame` →
//! `poll_bytes`.
//!
//! Scope and design: see `adr/0003-webm-mux.md`. Mirrors `iso_bmff::mux`'s
//! typestate shape (`Open` → `Live`) and `output`/`poll_bytes` drain pattern.

#![forbid(unsafe_code)]

use crate::types::TrackInfo;
use crate::{INLINE_TRACKS, MuxError, ids, vint};
use smallvec::SmallVec;
use std::marker::PhantomData;

/// Track registration phase — call [`Muxer::begin`] before pushing frames.
#[derive(Debug, Clone, Copy, Default)]
pub struct Open;

/// Streaming phase — `push_frame`/`poll_bytes` API.
#[derive(Debug, Clone, Copy, Default)]
pub struct Live;

/// Default frames buffered per `Cluster` before it is flushed to `output`.
pub const DEFAULT_CLUSTER_BATCH: usize = 32;

/// Matroska/`WebM` default `TimecodeScale` (ns per tick) — matches the
/// demux side's [`crate::demux`] default when a file omits it.
const DEFAULT_TIMECODE_SCALE: u64 = 1_000_000;

/// A `SimpleBlock`'s relative timecode is a signed 16-bit offset from its
/// `Cluster`'s `Timecode` — a frame whose offset would overflow this range
/// forces a new `Cluster` early, regardless of the batch size.
const fn fits_relative_timecode(delta: i64) -> bool {
    delta >= i16::MIN as i64 && delta <= i16::MAX as i64
}

/// Sans-IO `WebM` muxer. `S` is the typestate ([`Open`] or [`Live`]).
#[derive(Debug)]
pub struct Muxer<S = Open> {
    tracks: SmallVec<[TrackInfo; INLINE_TRACKS]>,
    timecode_scale: u64,
    output: Vec<u8>,
    output_consumed: usize,
    /// Buffered `SimpleBlock` element bytes for the still-open `Cluster`.
    cluster: Vec<u8>,
    /// Absolute timecode of the open `Cluster`'s first frame, if any.
    cluster_timecode: Option<i64>,
    cluster_frames: usize,
    batch: usize,
    _state: PhantomData<S>,
}

impl Muxer<Open> {
    /// Empty muxer in track-registration state, default `TimecodeScale`
    /// (1ms/tick) and [`DEFAULT_CLUSTER_BATCH`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_options(DEFAULT_TIMECODE_SCALE, DEFAULT_CLUSTER_BATCH)
    }

    /// Muxer with a custom `TimecodeScale` (ns/tick) and `Cluster` batch size.
    #[must_use]
    pub fn with_options(timecode_scale: u64, batch: usize) -> Self {
        Self {
            tracks: SmallVec::new(),
            timecode_scale: timecode_scale.max(1),
            output: Vec::with_capacity(4 * 1024),
            output_consumed: 0,
            cluster: Vec::new(),
            cluster_timecode: None,
            cluster_frames: 0,
            batch: batch.max(1),
            _state: PhantomData,
        }
    }

    /// Registered tracks so far.
    #[must_use]
    pub fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    /// Register a track. `track.track_number` must be non-zero and unique.
    ///
    /// # Errors
    ///
    /// [`MuxError::InvalidTrackNumber`] for `track_number == 0`;
    /// [`MuxError::DuplicateTrack`] if already registered.
    pub fn add_track(&mut self, track: TrackInfo) -> Result<(), MuxError> {
        if track.track_number == 0 {
            return Err(MuxError::InvalidTrackNumber);
        }
        if self
            .tracks
            .iter()
            .any(|t| t.track_number == track.track_number)
        {
            return Err(MuxError::DuplicateTrack(track.track_number));
        }
        self.tracks.push(track);
        Ok(())
    }

    /// Lock tracks, write the `EBML` header + `Segment`/`Info`/`Tracks`
    /// headers, and enter the live streaming state.
    #[must_use]
    pub fn begin(mut self) -> Muxer<Live> {
        write_ebml_header(&mut self.output);
        write_id(&mut self.output, ids::SEGMENT);
        vint::encode_unknown_size(4, &mut self.output); // streaming: total length unknown upfront
        write_segment_info(&mut self.output, self.timecode_scale);
        write_tracks(&mut self.output, &self.tracks);
        Muxer {
            tracks: self.tracks,
            timecode_scale: self.timecode_scale,
            output: self.output,
            output_consumed: self.output_consumed,
            cluster: self.cluster,
            cluster_timecode: self.cluster_timecode,
            cluster_frames: self.cluster_frames,
            batch: self.batch,
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
    /// Registered tracks.
    #[must_use]
    pub fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    /// `TimecodeScale` in effect (ns per tick) — frame `timecode` values are
    /// in these ticks, same unit [`crate::Demuxer::time_base`] reports.
    #[must_use]
    pub const fn timecode_scale(&self) -> u64 {
        self.timecode_scale
    }

    /// Push one frame. `timecode` is absolute, in `TimecodeScale` ticks, and
    /// must be non-decreasing per track (not enforced — an out-of-order
    /// timecode either still fits the open `Cluster`'s relative-offset range
    /// or forces a new `Cluster`, same as any large forward jump).
    ///
    /// Frames are buffered into the open `Cluster` and only reach `output`
    /// when the `Cluster` closes ([`Self::flush`], the batch/range limit, or
    /// the next `push_frame` that can't share the open `Cluster`) — call
    /// [`Self::poll_bytes`] after to drain.
    ///
    /// # Errors
    ///
    /// [`MuxError::UnknownTrack`] if `track_number` was never registered.
    pub fn push_frame(
        &mut self,
        track_number: u64,
        timecode: i64,
        is_keyframe: bool,
        payload: &[u8],
    ) -> Result<(), MuxError> {
        if !self.tracks.iter().any(|t| t.track_number == track_number) {
            return Err(MuxError::UnknownTrack(track_number));
        }
        let needs_new_cluster = match self.cluster_timecode {
            None => true,
            Some(base) => {
                self.cluster_frames >= self.batch || !fits_relative_timecode(timecode - base)
            }
        };
        if needs_new_cluster {
            self.close_cluster();
            self.cluster_timecode = Some(timecode);
        }
        // Base is always `Some` here — just set above, or the frame fit an
        // already-open cluster.
        let base = self.cluster_timecode.unwrap_or(timecode);
        write_simple_block(
            &mut self.cluster,
            track_number,
            (timecode - base) as i16,
            is_keyframe,
            payload,
        );
        self.cluster_frames += 1;
        Ok(())
    }

    /// Force the open `Cluster` (if any) to close and become available via
    /// [`Self::poll_bytes`]. Call before finishing the stream — an empty
    /// open `Cluster` (no frames pushed) writes nothing.
    pub fn flush(&mut self) {
        self.close_cluster();
    }

    /// Append available output bytes into `out`, same drain contract as
    /// `iso_bmff::mux::Muxer::poll_bytes`.
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

    fn close_cluster(&mut self) {
        if self.cluster_timecode.is_none() || self.cluster.is_empty() {
            self.cluster.clear();
            self.cluster_timecode = None;
            self.cluster_frames = 0;
            return;
        }
        let base = self.cluster_timecode.unwrap_or(0);
        let mut body = Vec::with_capacity(self.cluster.len() + 16);
        write_uint_elem(&mut body, ids::TIMECODE, base.max(0) as u64);
        body.extend_from_slice(&self.cluster);
        write_id(&mut self.output, ids::CLUSTER);
        vint::encode_size(body.len() as u64, &mut self.output);
        self.output.extend_from_slice(&body);
        self.cluster.clear();
        self.cluster_timecode = None;
        self.cluster_frames = 0;
    }
}

fn write_id(out: &mut Vec<u8>, id: u32) {
    vint::encode_id(id, out);
}

fn write_ebml_header(out: &mut Vec<u8>) {
    let mut body = Vec::new();
    write_uint_elem(&mut body, ids::EBML_VERSION, 1);
    write_uint_elem(&mut body, ids::EBML_READ_VERSION, 1);
    write_uint_elem(&mut body, ids::EBML_MAX_ID_LENGTH, 4);
    write_uint_elem(&mut body, ids::EBML_MAX_SIZE_LENGTH, 8);
    write_string_elem(&mut body, ids::DOC_TYPE, "webm");
    write_uint_elem(&mut body, ids::DOC_TYPE_VERSION, 2);
    write_uint_elem(&mut body, ids::DOC_TYPE_READ_VERSION, 2);
    write_id(out, ids::EBML_HEADER);
    vint::encode_size(body.len() as u64, out);
    out.extend_from_slice(&body);
}

fn write_segment_info(out: &mut Vec<u8>, timecode_scale: u64) {
    let mut body = Vec::new();
    write_uint_elem(&mut body, ids::TIMECODE_SCALE, timecode_scale);
    write_id(out, ids::SEGMENT_INFO);
    vint::encode_size(body.len() as u64, out);
    out.extend_from_slice(&body);
}

fn write_tracks(out: &mut Vec<u8>, tracks: &[TrackInfo]) {
    if tracks.is_empty() {
        return;
    }
    let mut body = Vec::new();
    for t in tracks {
        write_track_entry(&mut body, t);
    }
    write_id(out, ids::TRACKS);
    vint::encode_size(body.len() as u64, out);
    out.extend_from_slice(&body);
}

fn write_track_entry(out: &mut Vec<u8>, t: &TrackInfo) {
    let mut body = Vec::new();
    write_uint_elem(&mut body, ids::TRACK_NUMBER, t.track_number);
    write_uint_elem(&mut body, ids::TRACK_TYPE, u64::from(t.track_type));
    write_string_elem(&mut body, ids::CODEC_ID, &t.codec_id);
    if let Some(cp) = &t.codec_private {
        write_binary_elem(&mut body, ids::CODEC_PRIVATE, cp);
    }
    if t.is_video() {
        let mut video = Vec::new();
        write_uint_elem(&mut video, ids::PIXEL_WIDTH, u64::from(t.width));
        write_uint_elem(&mut video, ids::PIXEL_HEIGHT, u64::from(t.height));
        write_id(&mut body, ids::VIDEO);
        vint::encode_size(video.len() as u64, &mut body);
        body.extend_from_slice(&video);
    } else {
        let mut audio = Vec::new();
        write_float_elem(&mut audio, ids::SAMPLING_FREQUENCY, t.sample_rate);
        write_uint_elem(&mut audio, ids::CHANNELS, u64::from(t.channels));
        write_id(&mut body, ids::AUDIO);
        vint::encode_size(audio.len() as u64, &mut body);
        body.extend_from_slice(&audio);
    }
    write_id(out, ids::TRACK_ENTRY);
    vint::encode_size(body.len() as u64, out);
    out.extend_from_slice(&body);
}

/// `SimpleBlock` body: track number as a size-style VINT (marker stripped —
/// same convention [`crate::demux`]'s `parse_block_common` reads), 2-byte
/// signed relative timecode, 1 flags byte (`0x80` keyframe, no lacing bits
/// set — this muxer never lacs), then the raw payload.
fn write_simple_block(
    out: &mut Vec<u8>,
    track_number: u64,
    relative_timecode: i16,
    is_keyframe: bool,
    payload: &[u8],
) {
    let mut body = Vec::with_capacity(payload.len() + 4);
    vint::encode_size(track_number, &mut body);
    body.extend_from_slice(&relative_timecode.to_be_bytes());
    body.push(if is_keyframe { 0x80 } else { 0x00 });
    body.extend_from_slice(payload);
    write_id(out, ids::SIMPLE_BLOCK);
    vint::encode_size(body.len() as u64, out);
    out.extend_from_slice(&body);
}

/// Write a `UInt` master element: `id`, size, then the minimal big-endian
/// byte representation of `value` (leading zero bytes stripped; `0` itself
/// still writes one `0x00` byte — an empty-content `UInt` is legal EBML but
/// less common in the wild, and 1 byte costs nothing here).
fn write_uint_elem(out: &mut Vec<u8>, id: u32, value: u64) {
    let be = value.to_be_bytes();
    let first_nonzero = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
    write_id(out, id);
    vint::encode_size((be.len() - first_nonzero) as u64, out);
    out.extend_from_slice(&be[first_nonzero..]);
}

/// Write an EBML `Float` master element as 8 bytes (`f64`, big-endian) —
/// the demux side ([`crate::demux`]) accepts both 4- and 8-byte floats, so
/// writing the 8-byte form unconditionally is spec-valid and simpler.
fn write_float_elem(out: &mut Vec<u8>, id: u32, value: f64) {
    write_id(out, id);
    vint::encode_size(8, out);
    out.extend_from_slice(&value.to_be_bytes());
}

/// Write an ASCII `String` master element (`CodecID`, `DocType`) — content
/// bytes as-is, no null terminator (EBML strings are length-prefixed, not
/// C-style).
fn write_binary_elem(out: &mut Vec<u8>, id: u32, value: &[u8]) {
    write_id(out, id);
    vint::encode_size(value.len() as u64, out);
    out.extend_from_slice(value);
}

fn write_string_elem(out: &mut Vec<u8>, id: u32, value: &str) {
    write_id(out, id);
    vint::encode_size(value.len() as u64, out);
    out.extend_from_slice(value.as_bytes());
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod tests;
