//! Sans-IO `WebM`/Matroska demuxer: `push_bytes` → `poll_frame`.
//!
//! Element subset and known gaps (indefinite-`Cluster` lookahead, mux): see
//! `adr/0001-ebml-vint-webm-schema-v1.md` and
//! `adr/0002-full-matroska-profile.md`.

#![forbid(unsafe_code)]

use crate::lacing::{self, Lacing};
use crate::types::{Bytes, CuePoint, Frame, Rational, SeekEntry, TrackInfo};
use crate::{Error, INLINE_INDEX, INLINE_STACK, INLINE_TRACKS, ids, vint};
use smallvec::SmallVec;
use std::collections::VecDeque;

/// Matroska/`WebM` default `TimecodeScale` (ns per tick) when `Info` omits it.
const DEFAULT_TIMECODE_SCALE: u64 = 1_000_000;
/// `Audio\SamplingFrequency` spec default (Hz) when absent.
const DEFAULT_SAMPLE_RATE_HZ: f64 = 8000.0;
/// `Audio\Channels` spec default when absent.
const DEFAULT_CHANNELS: u32 = 1;

#[derive(Debug, Clone, Copy)]
struct OpenElement {
    id: u32,
    /// `None` = indefinite size; stays open until the parent closes or EOF.
    end: Option<usize>,
}

#[derive(Debug, Default)]
struct TrackScratch {
    track_number: Option<u64>,
    track_type: Option<u8>,
    codec_id: Option<String>,
    codec_private: Option<Bytes>,
    width: u32,
    height: u32,
    sample_rate: Option<f64>,
    channels: Option<u32>,
}

impl TrackScratch {
    /// A track without a number or codec ID is not usable — drop it rather
    /// than emit a half-populated [`TrackInfo`].
    fn finish(self) -> Option<TrackInfo> {
        Some(TrackInfo {
            track_number: self.track_number?,
            track_type: self.track_type.unwrap_or(0),
            codec_id: self.codec_id?,
            codec_private: self.codec_private,
            width: self.width,
            height: self.height,
            sample_rate: self.sample_rate.unwrap_or(DEFAULT_SAMPLE_RATE_HZ),
            channels: self.channels.unwrap_or(DEFAULT_CHANNELS),
        })
    }
}

/// A parsed `SimpleBlock`/`Block` before lace-splitting into `Frame`s.
///
/// `payloads` are copied out of `buffer` at parse time (not stored as
/// `(start, end)` offsets): a `Block` inside a `BlockGroup` is held here until
/// the group's closing tag is seen, which may span several `push_bytes`
/// calls — `compact()` can drop the underlying buffer prefix in the meantime,
/// so raw offsets would go stale (or point at already-discarded bytes).
#[derive(Debug)]
struct ParsedBlock {
    track_number: u64,
    timecode: i64,
    /// Raw flags byte — bit `0x80` is `SimpleBlock`'s keyframe flag (reserved,
    /// unused, in a `BlockGroup`'s `Block`).
    flags: u8,
    payloads: SmallVec<[Bytes; 8]>,
}

#[derive(Debug, Default)]
struct BlockGroupScratch {
    block: Option<ParsedBlock>,
    has_reference_block: bool,
    duration_ticks: Option<u64>,
}

#[derive(Debug, Default)]
struct CuePointScratch {
    time_ticks: Option<u64>,
    cluster_position: Option<u64>,
}

#[derive(Debug, Default)]
struct SeekScratch {
    id: Option<u32>,
    position: Option<u64>,
}

/// Sans-IO demuxer: `push_bytes` → `poll_frame`.
#[derive(Debug)]
pub struct Demuxer {
    buffer: Vec<u8>,
    read_pos: usize,
    stack: SmallVec<[OpenElement; INLINE_STACK]>,
    tracks: SmallVec<[TrackInfo; INLINE_TRACKS]>,
    building_track: Option<TrackScratch>,
    building_block_group: Option<BlockGroupScratch>,
    building_cue_point: Option<CuePointScratch>,
    building_seek: Option<SeekScratch>,
    cues: SmallVec<[CuePoint; INLINE_INDEX]>,
    seek_head: SmallVec<[SeekEntry; INLINE_INDEX]>,
    timecode_scale: u64,
    cluster_timecode: i64,
    frames: VecDeque<Frame>,
    /// Set once parsing hits a structurally unrecoverable position (reserved
    /// VINT, indefinite size on a non-descend element). No further bytes are
    /// interpreted; already-extracted tracks/frames are kept.
    halted: bool,
}

impl Default for Demuxer {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            read_pos: 0,
            stack: SmallVec::new(),
            tracks: SmallVec::new(),
            building_track: None,
            building_block_group: None,
            building_cue_point: None,
            building_seek: None,
            cues: SmallVec::new(),
            seek_head: SmallVec::new(),
            timecode_scale: DEFAULT_TIMECODE_SCALE,
            cluster_timecode: 0,
            frames: VecDeque::new(),
            halted: false,
        }
    }
}

impl Demuxer {
    /// Empty demuxer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed container bytes (sans-io; caller owns I/O).
    pub fn push_bytes(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
        self.pump();
        self.compact();
    }

    /// Tracks discovered so far (populated once `Tracks` has been parsed).
    #[must_use]
    pub fn streams(&self) -> &[TrackInfo] {
        &self.tracks
    }

    /// Next demuxed frame, if any.
    pub fn poll_frame(&mut self) -> Option<Frame> {
        self.frames.pop_front()
    }

    /// `Segment\Cues` entries parsed so far — informational seek index; this
    /// crate does not seek (sans-io: seeking is the I/O adapter's job).
    #[must_use]
    pub fn cues(&self) -> &[CuePoint] {
        &self.cues
    }

    /// `Segment\SeekHead` entries parsed so far — informational.
    #[must_use]
    pub fn seek_head(&self) -> &[SeekEntry] {
        &self.seek_head
    }

    /// Media timebase derived from `Segment\Info\TimecodeScale`
    /// (`1_000_000` ns/tick default per the Matroska/`WebM` spec).
    #[must_use]
    pub const fn time_base(&self) -> Rational {
        Rational::new(self.timecode_scale, 1_000_000_000)
    }

    fn pump(&mut self) {
        if self.halted {
            return;
        }
        loop {
            self.close_finished_contexts();
            if !self.step() {
                break;
            }
        }
    }

    fn close_finished_contexts(&mut self) {
        loop {
            let Some(top) = self.stack.last() else {
                return;
            };
            let done = matches!(top.end, Some(end) if self.read_pos >= end);
            if !done {
                return;
            }
            if let Some(closed) = self.stack.pop() {
                self.on_close(closed.id);
            }
        }
    }

    /// Process one element header at `read_pos`. `false` means stop pumping
    /// for now (incomplete buffer or halted).
    fn step(&mut self) -> bool {
        let (id, id_len) = match vint::decode_id(&self.buffer[self.read_pos..]) {
            Ok(v) => v,
            Err(Error::Incomplete) => return false,
            Err(Error::ReservedVint | Error::Unsupported(_)) => {
                self.halted = true;
                return false;
            }
        };
        self.close_indefinite_cluster_before_sibling(id);

        let (vs, size_len) = match vint::decode_size(&self.buffer[self.read_pos + id_len..]) {
            Ok(v) => v,
            Err(Error::Incomplete) => return false,
            Err(Error::ReservedVint | Error::Unsupported(_)) => {
                self.halted = true;
                return false;
            }
        };
        let header_len = id_len + size_len;
        let content_start = self.read_pos + header_len;
        let content_end = if vs.unknown {
            None
        } else {
            Some(content_start + vs.value as usize)
        };

        if ids::is_descend_master(id) {
            self.on_open(id);
            self.stack.push(OpenElement {
                id,
                end: content_end,
            });
            self.read_pos = content_start;
            return true;
        }

        let Some(end) = content_end else {
            // Indefinite size on an element we don't descend into: there is
            // no way to know where it ends. Unrecoverable — see adr/0001.
            self.halted = true;
            return false;
        };
        if end > self.buffer.len() {
            return false; // wait for more bytes
        }
        self.handle_leaf(id, content_start, end);
        self.read_pos = end;
        true
    }

    /// Sibling-ID lookahead (RFC 8794 §9.4): an indefinite-size `Cluster`
    /// has no explicit end, so it would otherwise stay open on the stack
    /// until `Segment` itself closes or EOF. When the next element ID is one
    /// that can only be a `Segment`-level child (`ids::is_segment_level_child`),
    /// it can't legally be a child of `Cluster` either — that's the signal
    /// this `Cluster` has implicitly ended; close it now instead of nesting
    /// the new element underneath it (which would grow the stack once per
    /// `Cluster` for the rest of the stream).
    fn close_indefinite_cluster_before_sibling(&mut self, id: u32) {
        if !ids::is_segment_level_child(id) {
            return;
        }
        while matches!(self.stack.last(), Some(top) if top.id == ids::CLUSTER && top.end.is_none())
        {
            if let Some(closed) = self.stack.pop() {
                self.on_close(closed.id);
            }
        }
    }

    fn on_open(&mut self, id: u32) {
        match id {
            ids::TRACK_ENTRY => self.building_track = Some(TrackScratch::default()),
            ids::CLUSTER => self.cluster_timecode = 0,
            ids::BLOCK_GROUP => self.building_block_group = Some(BlockGroupScratch::default()),
            ids::CUE_POINT => self.building_cue_point = Some(CuePointScratch::default()),
            ids::SEEK => self.building_seek = Some(SeekScratch::default()),
            _ => {}
        }
    }

    fn on_close(&mut self, id: u32) {
        match id {
            ids::TRACK_ENTRY => {
                if let Some(scratch) = self.building_track.take()
                    && let Some(track) = scratch.finish()
                {
                    self.tracks.push(track);
                }
            }
            ids::BLOCK_GROUP => self.finish_block_group(),
            ids::CUE_POINT => {
                if let Some(scratch) = self.building_cue_point.take()
                    && let (Some(time_ticks), Some(cluster_position)) =
                        (scratch.time_ticks, scratch.cluster_position)
                {
                    self.cues.push(CuePoint {
                        time_ticks,
                        cluster_position,
                    });
                }
            }
            ids::SEEK => {
                if let Some(scratch) = self.building_seek.take()
                    && let (Some(seek_id), Some(position)) = (scratch.id, scratch.position)
                {
                    self.seek_head.push(SeekEntry {
                        id: seek_id,
                        position,
                    });
                }
            }
            _ => {}
        }
    }

    fn finish_block_group(&mut self) {
        let Some(scratch) = self.building_block_group.take() else {
            return;
        };
        let Some(block) = scratch.block else {
            return;
        };
        let is_keyframe = !scratch.has_reference_block;
        for payload in block.payloads {
            self.frames.push_back(Frame {
                track_number: block.track_number,
                timecode: block.timecode,
                is_keyframe,
                duration_ticks: scratch.duration_ticks,
                payload,
            });
        }
    }

    fn top_is(&self, id: u32) -> bool {
        matches!(self.stack.last(), Some(top) if top.id == id)
    }

    fn handle_leaf(&mut self, id: u32, start: usize, end: usize) {
        match id {
            ids::TIMECODE_SCALE => {
                if let Some(v) = read_uint(&self.buffer[start..end]) {
                    self.timecode_scale = v.max(1);
                }
            }
            ids::TRACK_NUMBER => {
                let v = read_uint(&self.buffer[start..end]);
                if let (Some(v), Some(scratch)) = (v, self.building_track.as_mut()) {
                    scratch.track_number = Some(v);
                }
            }
            ids::TRACK_TYPE => {
                let v = read_uint(&self.buffer[start..end]);
                if let (Some(v), Some(scratch)) = (v, self.building_track.as_mut()) {
                    scratch.track_type = Some(v as u8);
                }
            }
            ids::CODEC_ID => {
                // clone: CodecID bytes live in the shared parse buffer; TrackInfo needs an owned String.
                let codec_id = String::from_utf8(self.buffer[start..end].to_vec()).ok();
                if let Some(scratch) = self.building_track.as_mut() {
                    scratch.codec_id = codec_id;
                }
            }
            ids::CODEC_PRIVATE => {
                // clone: bytes live in the shared parse buffer; TrackInfo needs an owned copy.
                let cp = Bytes::copy_from_slice(&self.buffer[start..end]);
                if let Some(scratch) = self.building_track.as_mut() {
                    scratch.codec_private = Some(cp);
                }
            }
            ids::PIXEL_WIDTH if self.top_is(ids::VIDEO) => {
                let v = read_uint(&self.buffer[start..end]);
                if let (Some(v), Some(scratch)) = (v, self.building_track.as_mut()) {
                    scratch.width = v as u32;
                }
            }
            ids::PIXEL_HEIGHT if self.top_is(ids::VIDEO) => {
                let v = read_uint(&self.buffer[start..end]);
                if let (Some(v), Some(scratch)) = (v, self.building_track.as_mut()) {
                    scratch.height = v as u32;
                }
            }
            ids::SAMPLING_FREQUENCY if self.top_is(ids::AUDIO) => {
                let v = read_float(&self.buffer[start..end]);
                if let (Some(v), Some(scratch)) = (v, self.building_track.as_mut()) {
                    scratch.sample_rate = Some(v);
                }
            }
            ids::CHANNELS if self.top_is(ids::AUDIO) => {
                let v = read_uint(&self.buffer[start..end]);
                if let (Some(v), Some(scratch)) = (v, self.building_track.as_mut()) {
                    scratch.channels = Some(v as u32);
                }
            }
            ids::TIMECODE => {
                if let Some(v) = read_uint(&self.buffer[start..end]) {
                    self.cluster_timecode = v as i64;
                }
            }
            ids::SIMPLE_BLOCK => self.handle_simple_block(start, end),
            ids::BLOCK if self.building_block_group.is_some() => self.handle_block(start, end),
            ids::BLOCK_DURATION if self.building_block_group.is_some() => {
                let v = read_uint(&self.buffer[start..end]);
                if let Some(scratch) = self.building_block_group.as_mut() {
                    scratch.duration_ticks = v;
                }
            }
            ids::REFERENCE_BLOCK if self.building_block_group.is_some() => {
                if let Some(scratch) = self.building_block_group.as_mut() {
                    scratch.has_reference_block = true;
                }
            }
            ids::CUE_TIME if self.top_is(ids::CUE_POINT) => {
                let v = read_uint(&self.buffer[start..end]);
                if let Some(scratch) = self.building_cue_point.as_mut() {
                    scratch.time_ticks = v;
                }
            }
            ids::CUE_CLUSTER_POSITION if self.top_is(ids::CUE_TRACK_POSITIONS) => {
                let v = read_uint(&self.buffer[start..end]);
                if let Some(scratch) = self.building_cue_point.as_mut() {
                    scratch.cluster_position = v;
                }
            }
            ids::SEEK_ID if self.top_is(ids::SEEK) => {
                let v = read_uint(&self.buffer[start..end]);
                if let (Some(v), Some(scratch)) = (v, self.building_seek.as_mut()) {
                    scratch.id = Some(v as u32);
                }
            }
            ids::SEEK_POSITION if self.top_is(ids::SEEK) => {
                let v = read_uint(&self.buffer[start..end]);
                if let Some(scratch) = self.building_seek.as_mut() {
                    scratch.position = v;
                }
            }
            _ => {}
        }
    }

    /// Parse a block's common wire format (track number VINT + 2-byte signed
    /// relative timecode + flags byte, optionally laced). Returns `None` on a
    /// truncated/malformed block.
    fn parse_block_common(&self, start: usize, end: usize) -> Option<ParsedBlock> {
        let body = &self.buffer[start..end];
        let (track_number, tn_len) = vint::decode_size(body).ok()?;
        if body.len() < tn_len + 3 {
            return None;
        }
        let rel_tc = i16::from_be_bytes([body[tn_len], body[tn_len + 1]]);
        let flags = body[tn_len + 2];
        let lacing = Lacing::from_flags(flags);
        let ranges = lacing::split(body, tn_len + 3, lacing)?;
        let timecode = self.cluster_timecode.saturating_add(i64::from(rel_tc));
        // `ranges` are relative to `body` (i.e. `start`); copy payload bytes out
        // now, while they're still guaranteed present in `buffer` — see
        // `ParsedBlock` doc comment for why offsets alone aren't safe to defer.
        let payloads = ranges
            .into_iter()
            .map(|(s, e)| Bytes::copy_from_slice(&self.buffer[start + s..start + e]))
            .collect();
        Some(ParsedBlock {
            track_number: track_number.value,
            timecode,
            flags,
            payloads,
        })
    }

    fn handle_simple_block(&mut self, start: usize, end: usize) {
        let Some(block) = self.parse_block_common(start, end) else {
            return;
        };
        let is_keyframe = block.flags & 0x80 != 0;
        for payload in block.payloads {
            self.frames.push_back(Frame {
                track_number: block.track_number,
                timecode: block.timecode,
                is_keyframe,
                duration_ticks: None,
                payload,
            });
        }
    }

    fn handle_block(&mut self, start: usize, end: usize) {
        let Some(block) = self.parse_block_common(start, end) else {
            return;
        };
        if let Some(scratch) = self.building_block_group.as_mut() {
            scratch.block = Some(block);
        }
    }

    fn compact(&mut self) {
        let drained = self.read_pos;
        if drained == 0 {
            return;
        }
        if drained < 64 * 1024 && drained * 2 < self.buffer.len() {
            return;
        }
        self.buffer.drain(..drained);
        for open in &mut self.stack {
            if let Some(end) = open.end.as_mut() {
                *end -= drained;
            }
        }
        self.read_pos = 0;
    }
}

/// Matroska/`WebM` "Unsigned Integer" element: big-endian, up to 8 bytes.
fn read_uint(body: &[u8]) -> Option<u64> {
    if body.len() > 8 {
        return None;
    }
    let mut v = 0u64;
    for &b in body {
        v = (v << 8) | u64::from(b);
    }
    Some(v)
}

/// Matroska/`WebM` "Float" element: 4-byte (f32) or 8-byte (f64) IEEE-754 big-endian.
fn read_float(body: &[u8]) -> Option<f64> {
    match body.len() {
        4 => Some(f64::from(f32::from_be_bytes(body.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(body.try_into().ok()?)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "demux_tests.rs"]
mod tests;
