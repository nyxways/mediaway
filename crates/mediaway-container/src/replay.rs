//! Replay ring — the last *N* seconds of encoded packets, cut only where a decoder can start.
//!
//! Sans-io bookkeeping over [`Packet`]s: no I/O, no clock. The caller pushes packets as an
//! encoder produces them, and asks for "the last *N* seconds" when someone wants a clip. The
//! answer is a packet sequence a fresh muxer can write as a standalone file.
//!
//! Decisions and their reasons: `crates/mediaway-container/adr/0004-replay-ring.md`. In short:
//!
//! - **One anchor stream** (normally video) decides where a clip may start: only at one of
//!   its keyframes. Everything else (audio) is cut by time to match.
//! - **Decode order, not presentation order.** Eviction and cut selection use `dts`. A clip
//!   starts at keyframe *K*, keeps anchor packets decoded from *K* on, and drops the leading
//!   pictures that present before *K* (their references may be on the far side of the cut).
//! - **Whole GOPs are evicted**, and only once the *next* keyframe is also out of the window,
//!   so a cut point always exists at or before the window's start. A clip can therefore begin
//!   up to one keyframe interval earlier than asked; it never begins later.
//! - **Memory is bounded** by an optional byte ceiling that overrides the window.
//!
//! By default payloads are [`Bytes`], so a clip costs a refcount per packet, not a copy of the
//! stream. A caller that already writes the packets to disk can hold a [`StoredPayload`] (where
//! the bytes are) instead, and read them back itself when it saves a clip: the ring is generic
//! over its payload ([`ReplayPayload`]) and does the same bookkeeping either way.

use std::collections::VecDeque;
use std::time::Duration;

use mediaway_common::{Bytes, Packet, Rational};

/// Why the ring refused something.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ReplayError {
    /// A packet for a stream that was never added.
    #[error("stream {stream_id} was not added to the replay ring")]
    UnknownStream {
        /// The packet's stream.
        stream_id: u32,
    },
    /// [`ReplayRing::add_stream`] for a stream that is already there.
    #[error("stream {stream_id} is already in the replay ring")]
    DuplicateStream {
        /// The stream.
        stream_id: u32,
    },
    /// A timebase with a zero numerator or denominator.
    #[error("stream {stream_id} has a degenerate timebase {num}/{den}")]
    BadTimebase {
        /// The stream.
        stream_id: u32,
        /// Numerator.
        num: u64,
        /// Denominator.
        den: u32,
    },
    /// A stream's decode timestamps went backwards. The ring is decode-ordered per stream, and
    /// accepting this would silently break both eviction and cutting.
    #[error("stream {stream_id}: dts {got} after {previous}; packets must arrive in decode order")]
    OutOfOrder {
        /// The stream.
        stream_id: u32,
        /// The last dts accepted on it.
        previous: i64,
        /// The dts that was refused.
        got: i64,
    },
}

/// A packet payload the ring can hold. [`ReplayRing::with_max_bytes`] counts [`Self::byte_len`].
pub trait ReplayPayload {
    /// The payload's size in bytes.
    fn byte_len(&self) -> usize;
}

impl ReplayPayload for Bytes {
    fn byte_len(&self) -> usize {
        self.len()
    }
}

/// Where a packet's payload was stored on the caller's disk.
///
/// For a caller that keeps packets in files and only their locations in the ring. The ring
/// never reads it: `file` is the caller's own id for a file, and `offset`/`len` the payload's
/// bytes in it (e.g. from an MP4 muxer's placements, `mp4::Muxer::poll_placements`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoredPayload {
    /// The caller's id for the file the bytes are in.
    pub file: u32,
    /// Byte offset of the payload in that file.
    pub offset: u64,
    /// Payload length in bytes.
    pub len: u32,
}

impl ReplayPayload for StoredPayload {
    fn byte_len(&self) -> usize {
        usize::try_from(self.len).unwrap_or(usize::MAX)
    }
}

/// Everything about a [`Packet`] except its payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PacketMeta {
    /// See [`Packet::stream_id`].
    pub stream_id: u32,
    /// See [`Packet::pts`].
    pub pts: i64,
    /// See [`Packet::dts`].
    pub dts: i64,
    /// See [`Packet::duration`].
    pub duration: u64,
    /// See [`Packet::is_keyframe`].
    pub is_keyframe: bool,
    /// See [`Packet::is_discard`].
    pub is_discard: bool,
}

impl PacketMeta {
    /// A packet with these fields and `payload`.
    #[must_use]
    pub const fn into_packet(self, payload: Bytes) -> Packet {
        Packet {
            stream_id: self.stream_id,
            pts: self.pts,
            dts: self.dts,
            duration: self.duration,
            is_keyframe: self.is_keyframe,
            is_discard: self.is_discard,
            payload,
        }
    }
}

impl From<&Packet> for PacketMeta {
    fn from(p: &Packet) -> Self {
        Self {
            stream_id: p.stream_id,
            pts: p.pts,
            dts: p.dts,
            duration: p.duration,
            is_keyframe: p.is_keyframe,
            is_discard: p.is_discard,
        }
    }
}

/// Nanoseconds on a shared axis, so streams with different timebases can be compared.
type Nanos = i128;

const NANOS_PER_SEC: i128 = 1_000_000_000;

/// `ticks` in `timebase` as nanoseconds, rounded toward negative infinity.
fn to_nanos(ticks: i64, timebase: Rational) -> Nanos {
    (i128::from(ticks) * i128::from(timebase.num) * NANOS_PER_SEC)
        .div_euclid(i128::from(timebase.den))
}

/// `nanos` as ticks in `timebase`, rounded to nearest.
fn to_ticks(nanos: Nanos, timebase: Rational) -> i64 {
    let scale = i128::from(timebase.num) * NANOS_PER_SEC;
    let ticks = (nanos * i128::from(timebase.den) + scale / 2).div_euclid(scale);
    i64::try_from(ticks).unwrap_or(if ticks < 0 { i64::MIN } else { i64::MAX })
}

fn duration_nanos(d: Duration) -> Nanos {
    Nanos::try_from(d.as_nanos()).unwrap_or(Nanos::MAX)
}

/// One held packet.
#[derive(Debug)]
struct Entry<P> {
    meta: PacketMeta,
    payload: P,
}

/// A decode-ordered queue of one stream's packets.
#[derive(Debug)]
struct Track<P> {
    id: u32,
    timebase: Rational,
    entries: VecDeque<Entry<P>>,
}

impl<P> Track<P> {
    fn last_dts(&self) -> Option<i64> {
        self.entries.back().map(|e| e.meta.dts)
    }
}

/// A keyframe of the anchor stream: somewhere a clip may start.
#[derive(Debug, Clone, Copy)]
struct CutPoint {
    dts: i64,
    pts: i64,
}

/// The last *N* seconds of encoded packets for one or more streams.
///
/// `P` is what the ring keeps of each payload: [`Bytes`] by default (built with
/// [`ReplayRing::new`], fed with [`ReplayRing::push`], read with [`Clip::packets`]), or any
/// [`ReplayPayload`] such as [`StoredPayload`] (built with [`ReplayRing::for_payload`], fed with
/// [`ReplayRing::push_entry`], read with [`Clip::entries`]).
///
/// ```
/// use std::time::Duration;
/// use mediaway_common::{Bytes, Packet, Rational};
/// use mediaway_container::replay::ReplayRing;
///
/// let ms = Rational::new(1, 1000);
/// let mut ring = ReplayRing::new(1, ms, Duration::from_secs(10))?;
/// for i in 0..20 {
///     ring.push(Packet {
///         stream_id: 1,
///         pts: i * 1000,
///         dts: i * 1000,
///         duration: 1000,
///         is_keyframe: i % 5 == 0,
///         is_discard: false,
///         payload: Bytes::from_static(b"frame"),
///     })?;
/// }
/// // Asking for 3 s starts at the keyframe before t=16 s: t=15 s, rebased to zero.
/// let clip = ring.clip_last(Duration::from_secs(3)).expect("a keyframe exists");
/// let dts: Vec<i64> = clip.packets().map(|p| p.dts).collect();
/// assert_eq!(dts, [0, 1000, 2000, 3000, 4000]);
/// # Ok::<(), mediaway_container::replay::ReplayError>(())
/// ```
#[derive(Debug)]
pub struct ReplayRing<P = Bytes> {
    /// `tracks[0]` is the anchor. A `Vec`, not a `SmallVec`: it is allocated once, when the
    /// ring is built, and never on the packet path.
    tracks: Vec<Track<P>>,
    /// The anchor's keyframes still in the ring, oldest first.
    cuts: VecDeque<CutPoint>,
    window: Nanos,
    max_bytes: Option<usize>,
    /// Payload bytes currently held, across every stream.
    bytes: usize,
    /// The newest decode time pushed on any stream.
    newest: Option<Nanos>,
}

impl ReplayRing {
    /// A ring keeping `window` of history, cut at keyframes of stream `anchor`, holding
    /// payloads as [`Bytes`]. For another payload type, see [`ReplayRing::for_payload`].
    ///
    /// # Errors
    ///
    /// [`ReplayError::BadTimebase`] for a zero numerator or denominator.
    pub fn new(anchor: u32, timebase: Rational, window: Duration) -> Result<Self, ReplayError> {
        Self::for_payload(anchor, timebase, window)
    }

    /// Add a packet, then evict whatever has fallen out of the window or over the ceiling.
    ///
    /// Anchor packets that arrive before the anchor's first keyframe are dropped: nothing can
    /// be decoded from them, so no clip could ever use them.
    ///
    /// # Errors
    ///
    /// [`ReplayError::UnknownStream`], or [`ReplayError::OutOfOrder`] if this stream's `dts`
    /// went backwards. The packet is not added in either case.
    pub fn push(&mut self, packet: Packet) -> Result<(), ReplayError> {
        let meta = PacketMeta::from(&packet);
        self.push_entry(meta, packet.payload)
    }
}

impl<P: ReplayPayload> ReplayRing<P> {
    /// [`ReplayRing::new`] for any payload type, e.g.
    /// `ReplayRing::<StoredPayload>::for_payload(anchor, timebase, window)`.
    ///
    /// `new` itself is [`Bytes`]-only, as `HashMap::new` is `RandomState`-only, so that a bare
    /// `ReplayRing::new(..)` never needs a type annotation.
    ///
    /// # Errors
    ///
    /// [`ReplayError::BadTimebase`] for a zero numerator or denominator.
    pub fn for_payload(
        anchor: u32,
        timebase: Rational,
        window: Duration,
    ) -> Result<Self, ReplayError> {
        let mut ring = Self {
            tracks: Vec::with_capacity(2),
            cuts: VecDeque::new(),
            window: duration_nanos(window),
            max_bytes: None,
            bytes: 0,
            newest: None,
        };
        ring.add_stream(anchor, timebase)?;
        Ok(ring)
    }

    /// Also evict history, oldest GOP first, while more than `max_bytes` of payload is held.
    ///
    /// The newest GOP is always kept, so a single GOP larger than the ceiling can exceed it.
    ///
    /// Counts [`ReplayPayload::byte_len`]. For [`StoredPayload`] that bounds the caller's bytes
    /// on disk the ring still refers to, not memory: the ring itself then holds a fixed few
    /// dozen bytes per packet.
    #[must_use]
    pub const fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }

    /// Carry another stream (e.g. audio), cut by time to match the anchor.
    ///
    /// # Errors
    ///
    /// [`ReplayError::DuplicateStream`] or [`ReplayError::BadTimebase`].
    pub fn add_stream(&mut self, stream_id: u32, timebase: Rational) -> Result<(), ReplayError> {
        if timebase.num == 0 || timebase.den == 0 {
            return Err(ReplayError::BadTimebase {
                stream_id,
                num: timebase.num,
                den: timebase.den,
            });
        }
        if self.tracks.iter().any(|t| t.id == stream_id) {
            return Err(ReplayError::DuplicateStream { stream_id });
        }
        self.tracks.push(Track {
            id: stream_id,
            timebase,
            entries: VecDeque::new(),
        });
        Ok(())
    }

    /// [`ReplayRing::push`] for any payload type: a packet's metadata, and what to keep of its
    /// payload. Same rules as `push`: evicts after adding, and drops anchor packets that
    /// arrive before the anchor's first keyframe.
    ///
    /// # Errors
    ///
    /// [`ReplayError::UnknownStream`], or [`ReplayError::OutOfOrder`] if this stream's `dts`
    /// went backwards. The packet is not added in either case.
    pub fn push_entry(&mut self, packet: PacketMeta, payload: P) -> Result<(), ReplayError> {
        let index = self
            .tracks
            .iter()
            .position(|t| t.id == packet.stream_id)
            .ok_or(ReplayError::UnknownStream {
                stream_id: packet.stream_id,
            })?;
        let track = &mut self.tracks[index];
        if let Some(previous) = track.last_dts()
            && packet.dts < previous
        {
            return Err(ReplayError::OutOfOrder {
                stream_id: packet.stream_id,
                previous,
                got: packet.dts,
            });
        }
        if index == 0 {
            if packet.is_keyframe {
                self.cuts.push_back(CutPoint {
                    dts: packet.dts,
                    pts: packet.pts,
                });
            } else if self.cuts.is_empty() {
                return Ok(());
            }
        }
        let at = to_nanos(packet.dts, track.timebase);
        self.newest = Some(self.newest.map_or(at, |n| n.max(at)));
        self.bytes += payload.byte_len();
        track.entries.push_back(Entry {
            meta: packet,
            payload,
        });
        self.evict();
        Ok(())
    }

    /// Drop whole GOPs from the front while the second-oldest cut point is itself old enough
    /// to start a full-window clip, or while over the byte ceiling.
    fn evict(&mut self) {
        let Some(newest) = self.newest else { return };
        let anchor = self.tracks[0].timebase;
        while let Some(&next) = self.cuts.get(1) {
            let out_of_window = newest - to_nanos(next.dts, anchor) >= self.window;
            let over_budget = self.max_bytes.is_some_and(|max| self.bytes > max);
            if !(out_of_window || over_budget) {
                break;
            }
            self.cuts.pop_front();
            let (anchor_track, others) = self.tracks.split_at_mut(1);
            let anchor_track = &mut anchor_track[0];
            while anchor_track
                .entries
                .front()
                .is_some_and(|e| e.meta.dts < next.dts)
            {
                if let Some(e) = anchor_track.entries.pop_front() {
                    self.bytes -= e.payload.byte_len();
                }
            }
            let start = to_nanos(next.pts, anchor);
            for track in others {
                let tb = track.timebase;
                while track
                    .entries
                    .front()
                    .is_some_and(|e| to_nanos(e.meta.pts, tb) < start)
                {
                    if let Some(e) = track.entries.pop_front() {
                        self.bytes -= e.payload.byte_len();
                    }
                }
            }
        }
    }

    /// Payload bytes currently held: the sum of [`ReplayPayload::byte_len`].
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// Every payload the ring still holds, stream by stream, oldest first within a stream.
    ///
    /// For [`StoredPayload`] this says which stored bytes are still needed: storage no item
    /// refers to will never be handed out again (evicted and dropped packets are gone for
    /// good), so the caller may free it, e.g. delete a spill file whose id no longer appears.
    /// Entries the caller copied out of a [`Clip`] are its own to keep alive.
    pub fn payloads(&self) -> impl Iterator<Item = &P> + '_ {
        self.tracks
            .iter()
            .flat_map(|t| t.entries.iter().map(|e| &e.payload))
    }

    /// How far back the oldest cut point is from the newest packet, i.e. the longest clip
    /// [`Self::clip_last`] can return right now. Zero before the first keyframe.
    #[must_use]
    pub fn span(&self) -> Duration {
        match (self.cuts.front(), self.newest) {
            (Some(first), Some(newest)) => {
                let nanos = newest - to_nanos(first.dts, self.tracks[0].timebase);
                Duration::from_nanos(u64::try_from(nanos.max(0)).unwrap_or(u64::MAX))
            }
            _ => Duration::ZERO,
        }
    }

    /// The last `span` of every stream, starting at the latest anchor keyframe at or before
    /// `newest - span`. Starts earlier than asked by up to one keyframe interval; starts at
    /// the oldest keyframe held when the ring holds less than `span`.
    ///
    /// `None` until the anchor's first keyframe has been pushed.
    #[must_use]
    pub fn clip_last(&self, span: Duration) -> Option<Clip<'_, P>> {
        let newest = self.newest?;
        let anchor = self.tracks[0].timebase;
        let target = newest.saturating_sub(duration_nanos(span));
        let later = self
            .cuts
            .partition_point(|c| to_nanos(c.dts, anchor) <= target);
        let cut = *self.cuts.get(later.saturating_sub(1))?;
        Some(Clip {
            ring: self,
            cut,
            origin: to_nanos(cut.dts, anchor),
        })
    }
}

/// A cut of a [`ReplayRing`]: which packets, and their rebased timestamps.
///
/// Borrows the ring, so it has to be written out before the next [`ReplayRing::push`].
#[derive(Debug)]
pub struct Clip<'a, P = Bytes> {
    ring: &'a ReplayRing<P>,
    cut: CutPoint,
    /// The cut keyframe's decode time. Becomes zero in the clip.
    origin: Nanos,
}

// Not derived: a derive would require `P: Clone`/`P: Copy`, and a `Clip` only borrows its `P`s.
impl<P> Clone for Clip<'_, P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for Clip<'_, P> {}

impl<'a, P> Clip<'a, P> {
    /// The cut keyframe's `dts`, in the anchor's own timebase and on the timeline the packets
    /// were pushed on. Subtracting it maps a time on that timeline into the clip.
    #[must_use]
    pub const fn anchor_origin(&self) -> i64 {
        self.cut.dts
    }

    /// From the cut to the newest packet pushed.
    #[must_use]
    pub fn duration(&self) -> Duration {
        let nanos = self.ring.newest.unwrap_or(self.origin) - self.origin;
        Duration::from_nanos(u64::try_from(nanos.max(0)).unwrap_or(u64::MAX))
    }

    /// The clip's packets as `(metadata, payload)`, interleaved across streams in decode-time
    /// order, with `pts` and `dts` rebased so the cut keyframe decodes at zero: the sequence
    /// [`Clip::packets`] yields, for any payload type. Payloads are borrowed from the ring.
    #[must_use]
    pub fn entries(&self) -> ClipEntries<'a, P> {
        let ring = self.ring;
        let cut_pts = to_nanos(self.cut.pts, ring.tracks[0].timebase);
        let cursors = ring
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if i == 0 {
                    t.entries.partition_point(|e| e.meta.dts < self.cut.dts)
                } else {
                    t.entries
                        .partition_point(|e| to_nanos(e.meta.dts, t.timebase) < cut_pts)
                }
            })
            .collect();
        ClipEntries {
            ring,
            cut: self.cut,
            origin: self.origin,
            cursors,
        }
    }
}

impl<'a> Clip<'a> {
    /// The clip's packets, interleaved across streams in decode-time order, with `pts` and
    /// `dts` rebased so the cut keyframe decodes at zero.
    ///
    /// Each item clones a [`Packet`], whose payload is a refcounted `Bytes`: no payload copy.
    #[must_use]
    pub fn packets(&self) -> ClipPackets<'a> {
        ClipPackets {
            entries: self.entries(),
        }
    }
}

/// Iterator returned by [`Clip::entries`].
#[derive(Debug)]
pub struct ClipEntries<'a, P = Bytes> {
    ring: &'a ReplayRing<P>,
    cut: CutPoint,
    origin: Nanos,
    /// Next index per track. Allocated once per clip, not per packet.
    cursors: Vec<usize>,
}

impl<'a, P> Iterator for ClipEntries<'a, P> {
    type Item = (PacketMeta, &'a P);

    fn next(&mut self) -> Option<Self::Item> {
        let ring = self.ring;
        loop {
            // The track whose next packet decodes first. Ties go to the lower index, so the
            // anchor comes first.
            let (track, entry) = ring
                .tracks
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.entries.get(self.cursors[i]).map(|e| (i, e)))
                .min_by_key(|&(i, e)| (to_nanos(e.meta.dts, ring.tracks[i].timebase), i))?;
            self.cursors[track] += 1;
            // A leading picture of the cut: decoded after it, presented before it, and free to
            // reference the GOP that was cut away. Other streams need no such check: their
            // cursors start at the first packet decoded at or after the cut's presentation
            // time, and a packet never presents before it decodes.
            if track == 0 && entry.meta.pts < self.cut.pts {
                continue;
            }
            let timebase = ring.tracks[track].timebase;
            let offset = if track == 0 {
                self.cut.dts
            } else {
                to_ticks(self.origin, timebase)
            };
            let mut meta = entry.meta;
            meta.pts -= offset;
            meta.dts -= offset;
            return Some((meta, &entry.payload));
        }
    }
}

/// Iterator returned by [`Clip::packets`].
#[derive(Debug)]
pub struct ClipPackets<'a> {
    entries: ClipEntries<'a>,
}

impl Iterator for ClipPackets<'_> {
    type Item = Packet;

    fn next(&mut self) -> Option<Packet> {
        let (meta, payload) = self.entries.next()?;
        // clone: the ring keeps its copy for later clips; the payload is a refcounted `Bytes`,
        // so this is a refcount bump, not a copy of the frame.
        Some(meta.into_packet(payload.clone()))
    }
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
